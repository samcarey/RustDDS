use std::{
  collections::HashMap,
  sync::{Arc, RwLock},
};

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use mio_extras::channel as mio_channel;

use crate::{
  dds::{
    no_key,
    participant::DomainParticipantWeak,
    with_key::{DataSample, Sample, WriteOptionsBuilder},
  },
  qos, rpc,
  rtps::constant::{
    DiscoveryNotificationType, SECURE_BUILTIN_READERS_INIT_LIST, SECURE_BUILTIN_WRITERS_INIT_LIST,
  },
  security::{
    access_control::{ParticipantSecurityAttributes, PermissionsToken},
    authentication::{
      authentication_builtin::DiscHandshakeState, HandshakeMessageToken, IdentityToken,
      ValidationOutcome, GMCLASSID_SECURITY_AUTH_HANDSHAKE,
    },
    cryptographic::{
      CryptoToken, GMCLASSID_SECURITY_DATAREADER_CRYPTO_TOKENS,
      GMCLASSID_SECURITY_DATAWRITER_CRYPTO_TOKENS, GMCLASSID_SECURITY_PARTICIPANT_CRYPTO_TOKENS,
    },
    security_error,
    security_plugins::SecurityPluginsHandle,
    DataHolder, ParticipantGenericMessage, ParticipantSecurityInfo, ParticipantStatelessMessage,
    ParticipantVolatileMessageSecure, SecurityError, SecurityResult,
  },
  security_error, security_log,
  serialization::pl_cdr_adapters::PlCdrSerialize,
  structure::{
    entity::RTPSEntity,
    guid::{EntityId, GuidPrefix},
  },
  RepresentationIdentifier, SequenceNumber, GUID,
};
use super::{
  discovery_db::{discovery_db_read, discovery_db_write, DiscoveryDB},
  Participant_GUID, SpdpDiscoveredParticipantData,
};

// Enum indicating if secure discovery allows normal discovery to process
// something
#[derive(PartialEq)]
pub(crate) enum NormalDiscoveryPermission {
  Allow,
  Deny,
}

// Enum for authentication status of a remote participant
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum AuthenticationStatus {
  Authenticated,
  Authenticating, // In the process of being authenticated
  Unauthenticated, /* Not authenticated, but still allowed to communicate with in a limited way
                   * (see Security spec section 8.8.2.1) */
  Rejected, // Could not authenticate & should not communicate to
}

// How many times an authentication message is resent if we don't get an answer
const STORED_AUTH_MESSAGE_MAX_RESEND_COUNT: u8 = 10;

struct StoredAuthenticationMessage {
  message: ParticipantStatelessMessage,
  remaining_resend_counter: u8,
}

impl StoredAuthenticationMessage {
  pub fn new(message: ParticipantStatelessMessage) -> Self {
    Self {
      message,
      remaining_resend_counter: STORED_AUTH_MESSAGE_MAX_RESEND_COUNT,
    }
  }
}

// This struct is an appendix to Discovery that handles Security-related
// functionality. The intention is that Discovery calls the methods of this
// struct when Security matters needs to be handled.
// SecureDiscovery also stores items which Discovery needs to do security.
// Some local tokens etc. which do not change during runtime are stored here so
// they don't have to be fetched from security plugins every time when needed
pub(crate) struct SecureDiscovery {
  pub security_plugins: SecurityPluginsHandle,
  pub domain_id: u16,
  pub local_participant_guid: GUID,
  pub local_dp_identity_token: IdentityToken,
  pub local_dp_permissions_token: PermissionsToken,
  pub local_dp_property_qos: qos::policy::Property,
  pub local_dp_sec_attributes: ParticipantSecurityAttributes,

  generic_message_helper: ParticipantGenericMessageHelper,
  // SecureDiscovery maintains states of handshake with remote participants.
  // We use the same states as the built-in authentication plugin, since
  // SecureDiscovery currently supports the built-in plugin only.
  handshake_states: HashMap<GuidPrefix, DiscHandshakeState>,
  // Here we store the latest authentication message that we've sent to each remote,
  // in case they need to be sent again
  stored_authentication_messages: HashMap<GuidPrefix, StoredAuthenticationMessage>,
}

impl SecureDiscovery {
  pub fn new(
    domain_participant: &DomainParticipantWeak,
    security_plugins: SecurityPluginsHandle,
  ) -> Result<Self, &'static str> {
    // Run the Discovery-related initialization steps of DDS Security spec v1.1
    // Section "8.8.1 Authentication and AccessControl behavior with local
    // DomainParticipant"

    let mut plugins = security_plugins.lock().unwrap();

    let participant_guid_prefix = domain_participant.guid().prefix;

    let property_qos = domain_participant
      .qos()
      .property()
      .expect("No property QoS defined even though security is enabled");

    let identity_token = match plugins.get_identity_token(participant_guid_prefix) {
      Ok(token) => token,
      Err(_e) => {
        return Err("Could not get IdentityToken");
      }
    };

    let _identity_status_token = match plugins.get_identity_status_token(participant_guid_prefix) {
      Ok(token) => token,
      Err(_e) => {
        return Err("Could not get IdentityStatusToken");
      }
    };

    let permissions_token = match plugins.get_permissions_token(participant_guid_prefix) {
      Ok(token) => token,
      Err(_e) => {
        return Err("Could not get PermissionsToken");
      }
    };

    let credential_token = match plugins.get_permissions_credential_token(participant_guid_prefix) {
      Ok(token) => token,
      Err(_e) => {
        return Err("Could not get PermissionsCredentialToken");
      }
    };

    if plugins
      .set_permissions_credential_and_token(
        participant_guid_prefix,
        credential_token,
        permissions_token.clone(),
      )
      .is_err()
    {
      return Err("Could not set permission tokens.");
    }

    let security_attributes = match plugins.get_participant_sec_attributes(participant_guid_prefix)
    {
      Ok(val) => val,
      Err(_e) => {
        return Err("Could not get ParticipantSecurityAttributes");
      }
    };

    drop(plugins); // Drop mutex guard on plugins so that plugins can be moved to self

    Ok(Self {
      security_plugins,
      domain_id: domain_participant.domain_id(),
      local_participant_guid: domain_participant.guid(),
      local_dp_identity_token: identity_token,
      local_dp_permissions_token: permissions_token,
      local_dp_property_qos: property_qos,
      local_dp_sec_attributes: security_attributes,
      generic_message_helper: ParticipantGenericMessageHelper::new(),
      handshake_states: HashMap::new(),
      stored_authentication_messages: HashMap::new(),
    })
  }

  // Inspect a new sample from the standard DCPSParticipant Builtin Topic
  // Possibly start the authentication protocol
  // Return return value indicates if normal Discovery can process the sample as
  // usual
  pub fn participant_read(
    &mut self,
    ds: &DataSample<SpdpDiscoveredParticipantData>,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
    discovery_updated_sender: &mio_channel::SyncSender<DiscoveryNotificationType>,
    auth_msg_writer: &no_key::DataWriter<ParticipantStatelessMessage>,
  ) -> NormalDiscoveryPermission {
    match &ds.value {
      Sample::Value(participant_data) => self.participant_data_read(
        participant_data,
        discovery_db,
        discovery_updated_sender,
        auth_msg_writer,
      ),
      Sample::Dispose(participant_guid) => {
        self.participant_dispose_read(participant_guid, discovery_db)
      }
    }
  }

  // This function inspects a data message from normal DCPSParticipant topic
  // The authentication protocol is possibly started
  // The return value tells if normal Discovery is allowed to process
  // the message.
  #[allow(clippy::needless_bool)] // for return value clarity
  fn participant_data_read(
    &mut self,
    participant_data: &SpdpDiscoveredParticipantData,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
    discovery_updated_sender: &mio_channel::SyncSender<DiscoveryNotificationType>,
    auth_msg_writer: &no_key::DataWriter<ParticipantStatelessMessage>,
  ) -> NormalDiscoveryPermission {
    let guid_prefix = participant_data.participant_guid.prefix;

    // Our action depends on the current authentication status of the remote
    let auth_status_opt = discovery_db_read(discovery_db).get_authentication_status(guid_prefix);

    // Here we get an updated authentication status
    let updated_auth_status = match auth_status_opt {
      None => {
        // No prior info on this participant. Check compatibility
        let compatible = self.check_compatibility_with_remote_participant(participant_data);
        if compatible {
          // We're compatible. Try to authenticate with this participant
          // This returns a new authentication status
          self.start_authentication_with_remote(
            participant_data,
            discovery_db,
            discovery_updated_sender,
            auth_msg_writer,
          )
        } else {
          // We're not compatible Security-wise
          if self
            .local_dp_sec_attributes
            .allow_unauthenticated_participants
          {
            // But configuration still allows matching with the participant (in a limited
            // way)
            security_log!(
              "Remote participant has incompatible Security, but matching with it anyways, since \
               configuration allows this. Remote guid: {:?}",
              participant_data.participant_guid
            );
            AuthenticationStatus::Unauthenticated
          } else {
            // Not allowed to match
            security_log!(
              "Remote participant has incompatible Security, not matching with it. Remote guid: \
               {:?}",
              participant_data.participant_guid
            );
            AuthenticationStatus::Rejected
          }
        }
      }
      Some(AuthenticationStatus::Authenticating) => {
        // We are authenticating.
        // If we need to send this remote participant a handshake request but haven't
        // managed to do so, retry
        if let Some(DiscHandshakeState::PendingRequestSend) = self.get_handshake_state(&guid_prefix)
        {
          self.try_sending_new_handshake_request_message(
            guid_prefix,
            discovery_db,
            auth_msg_writer,
          );
        }
        info!("Authenticating with Participant {guid_prefix:?}");
        // Otherwise keep the same authentication status
        AuthenticationStatus::Authenticating
      }
      Some(other_status) => {
        // Do nothing, just keep the same status
        other_status
      }
    };

    // Update authentication status to DB
    discovery_db_write(discovery_db).update_authentication_status(guid_prefix, updated_auth_status);

    // Decide if normal Discovery can process the participant message
    if updated_auth_status == AuthenticationStatus::Unauthenticated
      || updated_auth_status == AuthenticationStatus::Authenticating
    {
      NormalDiscoveryPermission::Allow
    } else {
      NormalDiscoveryPermission::Deny
    }
  }

  // This function inspects a dispose message from normal DCPSParticipant topic
  // and decides whether to allow Discovery to process the message
  fn participant_dispose_read(
    &self,
    participant_guid: &Participant_GUID,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
  ) -> NormalDiscoveryPermission {
    let guid_prefix = participant_guid.0.prefix;

    let db = discovery_db_read(discovery_db);

    // Permission to process the message depends on the participant's authentication
    // status
    match db.get_authentication_status(guid_prefix) {
      None => {
        // No prior info on this participant. Let the dispose message be processed
        NormalDiscoveryPermission::Allow
      }
      Some(AuthenticationStatus::Unauthenticated) => {
        // Participant has been marked as Unauthenticated. Allow to process.
        NormalDiscoveryPermission::Allow
      }
      Some(other_status) => {
        debug!(
          "Received a dispose message from participant with authentication status: {:?}. \
           Ignoring. Participant guid prefix: {:?}",
          other_status, guid_prefix
        );
        // Do not allow with any other status
        NormalDiscoveryPermission::Deny
      }
    }
  }

  // Return boolean indicating if we're compatible with the remote participant
  fn check_compatibility_with_remote_participant(
    &self,
    remote_data: &SpdpDiscoveredParticipantData,
  ) -> bool {
    // 1. Check identity tokens
    if let Some(token) = remote_data.identity_token.as_ref() {
      // Class ID of identity tokens needs to be the same (Means they implement the
      // same authentication plugin)
      let my_class_id = self.local_dp_identity_token.class_id();
      let remote_class_id = token.class_id();

      if my_class_id != remote_class_id {
        info!(
          "Participants not compatible because of different IdentityToken class IDs. Local \
           id:{my_class_id}, remote id: {remote_class_id}"
        );
        return false;
      }
    } else {
      // Remote participant does not have identity token.
      info!("Participants not compatible because remote does not have IdentityToken");
      return false;
    }

    // 2. Check permission tokens
    if let Some(token) = remote_data.permissions_token.as_ref() {
      // Class ID of permission tokens needs to be the same (Means they implement the
      // same access control plugin)
      let my_class_id = self.local_dp_permissions_token.class_id();
      let remote_class_id = token.class_id();

      if my_class_id != remote_class_id {
        info!(
          "Participants not compatible because of different PermissionsToken class IDs. Local \
           id:{my_class_id}, remote id: {remote_class_id}"
        );
        return false;
      }
    } else {
      // Remote participant does not have a permissions token.
      info!("Participants not compatible because remote does not have PermissionsToken");
      return false;
    }

    // 3. Check security info (see Security specification section 7.2.7)
    if let Some(remote_sec_info) = remote_data.security_info.as_ref() {
      let my_sec_info = ParticipantSecurityInfo::from(self.local_dp_sec_attributes.clone());

      let my_mask = my_sec_info.participant_security_attributes;
      let remote_mask = remote_sec_info.participant_security_attributes;

      let my_plugin_mask = my_sec_info.plugin_participant_security_attributes;
      let remote_plugin_mask = remote_sec_info.plugin_participant_security_attributes;

      // From the spec:
      // "A compatible configuration is defined as having the same value for
      // all of the attributes in the ParticipantSecurityInfo".
      if my_mask.is_valid()
        && remote_mask.is_valid()
        && my_plugin_mask.is_valid()
        && remote_plugin_mask.is_valid()
      {
        // Check equality of security infos when all masks are valid
        if my_sec_info != *remote_sec_info {
          info!("Participants not compatible because of unequal ParticipantSecurityInfos");
          return false;
        }
      } else {
        // But also from the spec:
        // "If the is_valid is set to zero on either of the masks, the comparison
        // between the local and remote setting for the ParticipantSecurityInfo
        // shall ignore the attribute"

        // TODO: Does it actually make sense to ignore the masks if they're not valid?
        // Seems a bit strange. Currently we require that all masks are valid
        info!(
          "Participants not compatible because some ParticipantSecurityInfo masks are not valid"
        );
        return false;
      }
    } else {
      // Remote participant does not have security info.
      info!("Participants not compatible because remote does not have ParticipantSecurityInfo");
      return false;
    }

    // All checks passed: we are compatible
    true
  }

  // This function is called once we have discovered a new remote participant that
  // we're compatible with Security-wise.
  // It contains the first authentication steps described in section 8.8.2
  // "Authentication behavior with discovered DomainParticipant" of the Security
  // specification.
  // The function returns the resulting authentication status of the remote
  fn start_authentication_with_remote(
    &mut self,
    participant_data: &SpdpDiscoveredParticipantData,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
    discovery_updated_sender: &mio_channel::SyncSender<DiscoveryNotificationType>,
    auth_msg_writer: &no_key::DataWriter<ParticipantStatelessMessage>,
  ) -> AuthenticationStatus {
    // Gather some needed items
    let my_guid = self.local_participant_guid;
    let remote_guid = participant_data.participant_guid;
    let remote_identity_token = participant_data
      .identity_token
      .clone()
      .expect("IdentityToken disappeared"); // Identity token is here since compatibility test passed

    // First validate the remote identity
    let outcome: ValidationOutcome = match self
      .security_plugins
      .get_plugins()
      .validate_remote_identity(
        my_guid.prefix,
        remote_identity_token,
        remote_guid.prefix,
        None,
      ) {
      Ok(res) => {
        // Validation passed. Getting only the validation outcome, ignoring
        // authentication request token which is not used
        res.0
      }
      Err(e) => {
        // Validation failed
        security_log!(
          "Failed to validate the identity of a remote participant with guid: {:?}. Info: {}",
          remote_guid,
          e.msg
        );
        // See if we can treat the participant as Unauthenticated or should we reject it
        if self
          .local_dp_sec_attributes
          .allow_unauthenticated_participants
        {
          security_log!(
            "Treating the participant with guid {:?} as Unauthenticated, since configuration \
             allows this.",
            remote_guid,
          );
          return AuthenticationStatus::Unauthenticated;
        } else {
          // Reject the damn thing
          return AuthenticationStatus::Rejected;
        }
      }
    };

    info!(
      "Validated identity of remote participant with guid: {:?}",
      remote_guid
    );

    // Add remote participant to DiscoveryDB with status 'Authenticating' and notify
    // DP event loop. This will result in matching the builtin
    // ParticipantStatelessMessage endpoints, which are used for exchanging
    // authentication messages.
    discovery_db_write(discovery_db).update_participant(participant_data);
    self.update_participant_authentication_status_and_notify_dp(
      remote_guid.prefix,
      AuthenticationStatus::Authenticating,
      discovery_db,
      discovery_updated_sender,
    );

    // What is the exact validation outcome?
    // The returned authentication status is from this match statement
    match outcome {
      ValidationOutcome::PendingHandshakeRequest => {
        // We should send the handshake request
        self.update_handshake_state(remote_guid.prefix, DiscHandshakeState::PendingRequestSend);
        self.try_sending_new_handshake_request_message(
          remote_guid.prefix,
          discovery_db,
          auth_msg_writer,
        );

        AuthenticationStatus::Authenticating // return value
      }
      ValidationOutcome::PendingHandshakeMessage => {
        // We should wait for the handshake request
        self.update_handshake_state(
          remote_guid.prefix,
          DiscHandshakeState::PendingRequestMessage,
        );

        debug!(
          "Waiting for a handshake request from remote with guid {:?}",
          remote_guid
        );

        AuthenticationStatus::Authenticating // return value
      }
      outcome => {
        // Other outcomes should not be possible
        error!(
          "Got an unexpected outcome when validating remote identity. Validation outcome: {:?}. \
           Remote guid: {:?}",
          outcome, remote_guid
        );
        AuthenticationStatus::Rejected // return value
      }
    }
  }

  fn update_participant_authentication_status_and_notify_dp(
    &mut self,
    participant_guid_prefix: GuidPrefix,
    new_status: AuthenticationStatus,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
    discovery_updated_sender: &mio_channel::SyncSender<DiscoveryNotificationType>,
  ) {
    let mut db = discovery_db_write(discovery_db);
    db.update_authentication_status(participant_guid_prefix, new_status);

    send_discovery_notification(
      discovery_updated_sender,
      DiscoveryNotificationType::ParticipantUpdated {
        guid_prefix: participant_guid_prefix,
      },
    );
  }

  fn create_handshake_request_message(
    &mut self,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
    remote_guid_prefix: GuidPrefix,
  ) -> SecurityResult<ParticipantStatelessMessage> {
    // First get our own serialized data
    let my_ser_data = self.get_serialized_local_participant_data(discovery_db)?;

    // Get the handshake request token
    let (validation_outcome, request_token) = self
      .security_plugins
      .get_plugins()
      .begin_handshake_request(
        self.local_participant_guid.prefix,
        remote_guid_prefix,
        my_ser_data,
      )?;

    if validation_outcome != ValidationOutcome::PendingHandshakeMessage {
      // PendingHandshakeMessage is the only expected validation outcome
      return Err(security_error!(
        "Received an unexpected validation outcome from begin_handshake_request. Outcome: {:?}",
        validation_outcome
      ));
    }

    // Create the request message with the request token
    let request_message = self.new_stateless_message(
      GMCLASSID_SECURITY_AUTH_HANDSHAKE,
      remote_guid_prefix,
      None,
      request_token,
    );
    Ok(request_message)
  }

  fn try_sending_new_handshake_request_message(
    &mut self,
    remote_guid_prefix: GuidPrefix,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
    auth_msg_writer: &no_key::DataWriter<ParticipantStatelessMessage>,
  ) {
    debug!(
      "Send a handshake request message to remote with guid prefix: {:?}",
      remote_guid_prefix
    );

    let request_message =
      match self.create_handshake_request_message(discovery_db, remote_guid_prefix) {
        Ok(message) => message,
        Err(e) => {
          error!(
            "Failed to create a handshake request message. Reason: {}. Remote guid prefix: {:?}. \
             Trying again later.",
            e.msg, remote_guid_prefix
          );
          return;
        }
      };
    // Request was created successfully

    // Add the message to cache of unanswered messages so that we'll try
    // resending it later if needed
    self.stored_authentication_messages.insert(
      remote_guid_prefix,
      StoredAuthenticationMessage::new(request_message.clone()),
    );

    // Try to send the message
    let _ = auth_msg_writer.write(request_message, None).map_err(|err| {
      warn!(
        "Failed to send a handshake request message. Remote GUID prefix: {:?}. Info: {}. Trying \
         to resend the message later.",
        remote_guid_prefix, err
      );
    });

    // Update handshake state to pending reply message
    self.update_handshake_state(remote_guid_prefix, DiscHandshakeState::PendingReplyMessage);
  }

  pub fn resend_unanswered_authentication_messages(
    &mut self,
    auth_msg_writer: &no_key::DataWriter<ParticipantStatelessMessage>,
  ) {
    for (guid_prefix, stored_message) in self.stored_authentication_messages.iter_mut() {
      // Resend the message unless it's a final message (which needs to be requested
      // from us)
      if self.handshake_states.get(guid_prefix)
        != Some(&DiscHandshakeState::CompletedWithFinalMessageSent)
      {
        match auth_msg_writer.write(stored_message.message.clone(), None) {
          Ok(()) => {
            stored_message.remaining_resend_counter -= 1;
            debug!(
              "Resent an unanswered authentication message to remote with guid prefix {:?}. \
               Resending at most {} more times.",
              guid_prefix, stored_message.remaining_resend_counter,
            );
          }
          Err(err) => {
            debug!(
              "Failed to resend an unanswered authentication message to remote with guid prefix \
               {:?}. Error: {}. Retrying later.",
              guid_prefix, err
            );
          }
        }
      }
    }
    // Remove messages with no more resends
    self
      .stored_authentication_messages
      .retain(|_guid_prefix, message| message.remaining_resend_counter > 0);
  }

  fn reset_stored_message_resend_counter(&mut self, remote_guid_prefix: &GuidPrefix) {
    if let Some(msg) = self
      .stored_authentication_messages
      .get_mut(remote_guid_prefix)
    {
      msg.remaining_resend_counter = STORED_AUTH_MESSAGE_MAX_RESEND_COUNT;
    } else {
      debug!(
        "Did not find a stored message for remote with guid prefix {:?}",
        remote_guid_prefix
      );
    }
  }

  pub fn participant_stateless_message_read(
    &mut self,
    message: &ParticipantStatelessMessage,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
    discovery_updated_sender: &mio_channel::SyncSender<DiscoveryNotificationType>,
    auth_msg_writer: &no_key::DataWriter<ParticipantStatelessMessage>,
  ) {
    if !self.is_stateless_msg_for_local_participant(message) {
      trace!("Ignoring a ParticipantStatelessMessage, since its not meant for me.");
      return;
    }

    // Check that GenericMessageClassID is what we expect
    if message.generic.message_class_id != GMCLASSID_SECURITY_AUTH_HANDSHAKE {
      debug!(
        "Received a ParticipantStatelessMessage with an unknown GenericMessageClassID: {}",
        message.generic.message_class_id
      );
      return;
    }

    let remote_guid_prefix = message.generic.source_guid_prefix();
    // What to do depends on the handshake state with the remote participant
    match self.get_handshake_state(&remote_guid_prefix) {
      None => {
        trace!(
          "Received a handshake message from remote with guid prefix {:?}. Ignoring, since no \
           handshake going on.",
          remote_guid_prefix
        );
      }
      Some(DiscHandshakeState::PendingRequestSend) => {
        // Haven't yet managed to create a handshake request for this remote
        self.try_sending_new_handshake_request_message(
          remote_guid_prefix,
          discovery_db,
          auth_msg_writer,
        );
      }
      Some(DiscHandshakeState::PendingRequestMessage) => {
        self.handshake_on_pending_request_message(message, discovery_db, auth_msg_writer);
      }
      Some(DiscHandshakeState::PendingReplyMessage) => {
        self.handshake_on_pending_reply_message(
          message,
          discovery_db,
          auth_msg_writer,
          discovery_updated_sender,
        );
      }
      Some(DiscHandshakeState::PendingFinalMessage) => {
        self.handshake_on_pending_final_message(message, discovery_db, discovery_updated_sender);
      }
      Some(DiscHandshakeState::CompletedWithFinalMessageSent) => {
        // Handshake with this remote has completed by us sending the final
        // message. Send the message again in case the remote hasn't
        // received it
        debug!(
          "Resending a final handshake message to remote with guid prefix {:?}",
          remote_guid_prefix
        );
        self.resend_final_handshake_message(remote_guid_prefix, auth_msg_writer);
      }
      Some(DiscHandshakeState::CompletedWithFinalMessageReceived) => {
        trace!(
          "Received a handshake message from remote with guid prefix {:?}. Handshake with this \
           participant has already been completed by receiving the final message. Nothing for us \
           to do anymore.",
          remote_guid_prefix
        );
      }
    }
  }

  fn handshake_on_pending_request_message(
    &mut self,
    received_message: &ParticipantStatelessMessage,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
    auth_msg_writer: &no_key::DataWriter<ParticipantStatelessMessage>,
  ) {
    let remote_guid_prefix = received_message.generic.source_guid_prefix();
    debug!(
      "Received a handshake message from remote with guid prefix {:?}. Expecting a handshake \
       request message.",
      remote_guid_prefix
    );
    let local_guid_prefix = self.local_participant_guid.prefix;

    // Get the token from the message
    let handshake_token = match get_handshake_token_from_stateless_message(received_message) {
      Some(token) => token,
      None => {
        error!(
          "A ParticipantStatelessMessage does not contain a message token. Remote guid prefix: \
           {:?}",
          remote_guid_prefix
        );
        return;
      }
    };

    // Get my own data serialized
    let my_serialized_data =
      if let Ok(data) = self.get_serialized_local_participant_data(discovery_db) {
        data
      } else {
        error!(" Could not get serialized local participant data");
        return;
      };

    // Now call the security functionality
    let result = self.security_plugins.get_plugins().begin_handshake_reply(
      local_guid_prefix,
      remote_guid_prefix,
      handshake_token,
      my_serialized_data,
    );
    match result {
      Ok((ValidationOutcome::PendingHandshakeMessage, reply_token)) => {
        // Request token was OK and we got a reply token to send back
        // Create a ParticipantStatelessMessage with the token
        let reply_message = self.new_stateless_message(
          GMCLASSID_SECURITY_AUTH_HANDSHAKE,
          remote_guid_prefix,
          Some(received_message),
          reply_token,
        );

        debug!(
          "Send a handshake reply message to participant with guid prefix {:?}",
          remote_guid_prefix
        );

        // Send the token
        let _ = auth_msg_writer
          .write(reply_message.clone(), None)
          .map_err(|err| {
            error!(
              "Failed to send a handshake reply message. Remote GUID prefix: {:?}. Info: {}. \
               Trying to resend the message later.",
              remote_guid_prefix, err
            );
          });

        // Add request message to cache of unanswered messages so that we'll try
        // resending it later if needed
        self.stored_authentication_messages.insert(
          remote_guid_prefix,
          StoredAuthenticationMessage::new(reply_message),
        );

        // Set handshake state as pending final message
        self
          .handshake_states
          .insert(remote_guid_prefix, DiscHandshakeState::PendingFinalMessage);
      }
      Ok((other_outcome, _reply_token)) => {
        // Other outcomes should not be possible
        error!(
          "Unexpected validation outcome from begin_handshake_reply. Outcome: {:?}. Remote guid \
           prefix: {:?}",
          other_outcome, remote_guid_prefix
        );
      }
      Err(e) => {
        error!(
          "Replying to a handshake request failed: {}. Remote guid prefix: {:?}",
          e, remote_guid_prefix
        );
      }
    }
  }

  fn handshake_on_pending_reply_message(
    &mut self,
    received_message: &ParticipantStatelessMessage,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
    auth_msg_writer: &no_key::DataWriter<ParticipantStatelessMessage>,
    discovery_updated_sender: &mio_channel::SyncSender<DiscoveryNotificationType>,
  ) {
    let remote_guid_prefix = received_message.generic.source_guid_prefix();
    debug!(
      "Received a handshake message from remote with guid prefix {:?}. Expecting a handshake \
       reply message.",
      remote_guid_prefix
    );

    // Make sure that 'related message identity' in the received message matches
    // the message that we have sent to the remote
    if !self.check_is_stateless_msg_related_to_our_msg(received_message, remote_guid_prefix) {
      warn!(
        "Received handshake message that is not related to the message that we have sent. \
         Ignoring. Remote guid prefix: {:?}",
        remote_guid_prefix
      );
      return;
    }

    // Get the token from the message
    let handshake_token = match get_handshake_token_from_stateless_message(received_message) {
      Some(token) => token,
      None => {
        error!(
          "A ParticipantStatelessMessage does not contain a message token. Ignoring the message. \
           Remote guid prefix: {:?}",
          remote_guid_prefix
        );
        return;
      }
    };

    // Now call the security functionality
    let result = self
      .security_plugins
      .get_plugins()
      .process_handshake(remote_guid_prefix, handshake_token);
    match result {
      Ok((ValidationOutcome::OkFinalMessage, Some(final_message_token))) => {
        // Everything went OK. Still need to send the final message to remote.
        // Create a ParticipantStatelessMessage with the token
        let final_message = self.new_stateless_message(
          GMCLASSID_SECURITY_AUTH_HANDSHAKE,
          remote_guid_prefix,
          Some(received_message),
          final_message_token,
        );

        debug!(
          "Send a final handshake message to participant with guid prefix {:?}",
          remote_guid_prefix
        );

        // Send the token
        let _ = auth_msg_writer
          .write(final_message.clone(), None)
          .map_err(|err| {
            error!(
              "Failed to send a final handshake message. Remote GUID prefix: {:?}. Info: {}. \
               Trying to resend the message later.",
              remote_guid_prefix, err
            );
          });

        // Add final message to cache of unanswered messages so that we'll try
        // resending it later if needed
        self.stored_authentication_messages.insert(
          remote_guid_prefix,
          StoredAuthenticationMessage::new(final_message),
        );

        // Set handshake state as completed with final message
        self.handshake_states.insert(
          remote_guid_prefix,
          DiscHandshakeState::CompletedWithFinalMessageSent,
        );

        self.on_remote_participant_authenticated(
          remote_guid_prefix,
          discovery_db,
          discovery_updated_sender,
        );
      }
      Ok((other_outcome, _token_opt)) => {
        // Expected only OkFinalMessage outcome
        error!(
          "Received an unexpected validation outcome from the security plugins. Outcome: {:?}. \
           Remote guid prefix: {:?}",
          other_outcome, remote_guid_prefix
        );
      }
      Err(e) => {
        error!(
          "Validating handshake reply message failed. Error: {}. Remote guid prefix: {:?}",
          e, remote_guid_prefix
        );
        // Reset stored message resend counter, so our resends can't be depleted by
        // sending us incorrect messages
        self.reset_stored_message_resend_counter(&remote_guid_prefix);
      }
    }
  }

  fn handshake_on_pending_final_message(
    &mut self,
    received_message: &ParticipantStatelessMessage,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
    discovery_updated_sender: &mio_channel::SyncSender<DiscoveryNotificationType>,
  ) {
    let remote_guid_prefix = received_message.generic.source_guid_prefix();
    debug!(
      "Received a handshake message from remote with guid prefix {:?}. Expecting a final \
       handshake message",
      remote_guid_prefix
    );

    // Make sure that 'related message identity' in the received message matches
    // the message that we have sent to the remote
    if !self.check_is_stateless_msg_related_to_our_msg(received_message, remote_guid_prefix) {
      warn!(
        "Received handshake message that is not related to the message that we have sent. \
         Ignoring. Remote guid prefix: {:?}",
        remote_guid_prefix
      );
      return;
    }

    // Get the token from the message
    let handshake_token = match get_handshake_token_from_stateless_message(received_message) {
      Some(token) => token,
      None => {
        error!(
          "A ParticipantStatelessMessage does not contain a message token. Ignoring the message. \
           Remote guid prefix: {:?}",
          remote_guid_prefix
        );
        return;
      }
    };

    // Now call the security functionality
    let result = self
      .security_plugins
      .get_plugins()
      .process_handshake(remote_guid_prefix, handshake_token);
    match result {
      Ok((ValidationOutcome::Ok, None)) => {
        // Everything went OK

        // Set handshake state as completed with final message
        self.handshake_states.insert(
          remote_guid_prefix,
          DiscHandshakeState::CompletedWithFinalMessageReceived,
        );

        // Remove the stored reply message so it won't be resent
        self
          .stored_authentication_messages
          .remove(&remote_guid_prefix);

        info!("Authenticated successfully Participant {remote_guid_prefix:?}");

        self.on_remote_participant_authenticated(
          remote_guid_prefix,
          discovery_db,
          discovery_updated_sender,
        );
      }
      Ok((other_outcome, _token_opt)) => {
        // Expected only Ok outcome
        error!(
          "Received an unexpected validation outcome from the security plugins. Outcome: {:?}. \
           Remote guid prefix: {:?}",
          other_outcome, remote_guid_prefix
        );
      }
      Err(e) => {
        error!(
          "Validating final handshake message failed. Error: {}. Remote guid prefix: {:?}",
          e, remote_guid_prefix
        );
        // Reset stored message resend counter, so our resends can't be depleted by
        // sending us incorrect messages
        self.reset_stored_message_resend_counter(&remote_guid_prefix);
      }
    }
  }

  pub fn volatile_message_secure_read(&mut self, msg: &ParticipantVolatileMessageSecure) {
    // Check is the message meant to us (see 7.4.4.4 Destination of the
    // ParticipantVolatileMessageSecure of the spec)
    let dest_guid = msg.generic.destination_participant_guid;
    let is_for_us = (dest_guid == GUID::GUID_UNKNOWN) || (dest_guid == self.local_participant_guid);
    if !is_for_us {
      trace!(
        "Ignoring ParticipantVolatileMessageSecure message since it's not for us. dest_guid: {:?}",
        dest_guid
      );
      return;
    }

    // Get crypto tokens from message
    let crypto_tokens = msg
      .generic
      .message_data
      .iter()
      .map(|dh| CryptoToken::from(dh.clone()))
      .collect();

    match msg.generic.message_class_id.as_str() {
      GMCLASSID_SECURITY_PARTICIPANT_CRYPTO_TOKENS => {
        // Got participant crypto tokens, see "7.4.4.6.1 Data for message class
        // GMCLASS_SECURITY_PARTICIPANT_CRYPTO_TOKENS" of the security spec

        // Make sure destination_participant_guid is correct
        if dest_guid != self.local_participant_guid {
          debug!("Invalid destination participant guid, ignoring participant crypto tokens");
          return;
        }

        let remote_participant_guidp = msg.generic.message_identity.writer_guid.prefix;
        if let Err(e) = self
          .security_plugins
          .get_plugins()
          .set_remote_participant_crypto_tokens(
            self.local_participant_guid.prefix,
            remote_participant_guidp,
            crypto_tokens,
          )
        {
          security_error!(
            "Failed to set remote participant crypto tokens: {}. Remote: {:?}",
            e,
            remote_participant_guidp
          );
        } else {
          info!(
            "Set crypto tokens for remote participant {:?}",
            remote_participant_guidp
          );
        }
      }

      GMCLASSID_SECURITY_DATAWRITER_CRYPTO_TOKENS => {
        // Got data writer crypto tokens, see "7.4.4.6.2 Data for message class
        // GMCLASSID_SECURITY_DATAWRITER_CRYPTO_TOKENS" of the security spec

        if let Err(e) = self
          .security_plugins
          .get_plugins()
          .set_remote_writer_crypto_tokens(
            msg.generic.source_endpoint_guid,
            msg.generic.destination_endpoint_guid,
            crypto_tokens,
          )
        {
          security_error!(
            "Failed to set remote writer crypto tokens: {}. Remote: {:?}",
            e,
            msg.generic.source_endpoint_guid
          );
        } else {
          info!(
            "Set crypto tokens for remote writer {:?}",
            msg.generic.source_endpoint_guid
          );
        }
      }

      GMCLASSID_SECURITY_DATAREADER_CRYPTO_TOKENS => {
        // Got data reader crypto tokens, see "7.4.4.6.3 Data for message class
        // GMCLASSID_SECURITY_DATAREADER_CRYPTO_TOKENS" of the security spec

        if let Err(e) = self
          .security_plugins
          .get_plugins()
          .set_remote_reader_crypto_tokens(
            msg.generic.source_endpoint_guid,
            msg.generic.destination_endpoint_guid,
            crypto_tokens,
          )
        {
          security_error!(
            "Failed to set remote reader crypto tokens: {}. Remote: {:?}",
            e,
            msg.generic.source_endpoint_guid
          );
        } else {
          info!(
            "Set crypto tokens for remote reader {:?}",
            msg.generic.source_endpoint_guid
          );
        }
      }
      other => {
        debug!("Unknown message_class_id in a volatile message: {}", other);
      }
    }
  }

  fn on_remote_participant_authenticated(
    &mut self,
    remote_guid_prefix: GuidPrefix,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
    discovery_updated_sender: &mio_channel::SyncSender<DiscoveryNotificationType>,
  ) {
    security_log!(
      "Authenticated participant with GUID prefix {:?}",
      remote_guid_prefix
    );

    // Call the required access control methods
    // (see Security spec. section "8.8.6 AccessControl behavior with remote
    // participant discovery")
    match self.validate_remote_participant_permissions(remote_guid_prefix, discovery_db) {
      Ok(()) => {
        debug!(
          "Validated permissions for remote with guid prefix {:?}",
          remote_guid_prefix
        );
      }
      Err(e) => {
        security_log!(
          "Validating permissions for remote failed: {}. Rejecting the remote. Guid prefix: {:?}",
          e,
          remote_guid_prefix
        );
        discovery_db_write(discovery_db)
          .update_authentication_status(remote_guid_prefix, AuthenticationStatus::Rejected);
        return;
      }
    }

    // If needed, check is remote allowed to join the domain
    if self.local_dp_sec_attributes.is_access_protected {
      match self
        .security_plugins
        .get_plugins()
        .check_remote_participant(self.domain_id, remote_guid_prefix)
      {
        Ok(()) => {
          info!(
            "Allowing remote participant to join the domain. Remote guid prefix {:?}",
            remote_guid_prefix
          );
        }
        Err(e) => {
          security_log!(
            "Remote participant is not allowed to join the domain: {}. Rejecting the remote. Guid \
             prefix: {:?}",
            e,
            remote_guid_prefix
          );
          discovery_db_write(discovery_db)
            .update_authentication_status(remote_guid_prefix, AuthenticationStatus::Rejected);
          return;
        }
      }
    }
    // Permission checks OK

    // Update participant status as Authenticated & notify dp
    self.update_participant_authentication_status_and_notify_dp(
      remote_guid_prefix,
      AuthenticationStatus::Authenticated,
      discovery_db,
      discovery_updated_sender,
    );
  }

  // Initiates the exchange of cryptographic keys with the remote participant.
  // The exchange is started for the secure built-in topics.
  // Note that this function needs to be called after the built-in endpoints have
  // been matched in dp_event_loop, since otherwise the key exchange messages that
  // we send (in topic ParticipantVolatileMessageSecure) won't reach the remote
  // participant.
  pub fn start_key_exchange_with_remote(
    &mut self,
    remote_guid_prefix: GuidPrefix,
    key_exchange_writer: &no_key::DataWriter<ParticipantVolatileMessageSecure>,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
  ) {
    // Register remote participant to crypto plugin with the shared secret which
    // resulted from the successful handshake
    if let Err(e) = {
      let shared_secret = self
        .security_plugins
        .get_plugins()
        .get_shared_secret(remote_guid_prefix); // Release lock
      shared_secret.and_then(|shared_secret| {
        self
          .security_plugins
          .get_plugins()
          .register_matched_remote_participant(
            self.local_participant_guid.prefix,
            remote_guid_prefix,
            shared_secret,
          )
      })
    } {
      security_error!(
        "Failed to register remote participant with the crypto plugin: {}. Remote: {:?}",
        e,
        remote_guid_prefix
      );
      return;
    } else {
      info!(
        "Registered remote participant with the crypto plugin. Remote = {:?}",
        remote_guid_prefix
      );
    }

    // Read remote's available endpoints from DB
    let remotes_builtin_endpoints =
      match discovery_db_read(discovery_db).find_participant_proxy(remote_guid_prefix) {
        Some(data) => data.available_builtin_endpoints,
        None => {
          error!(
            "Could not find participant {:?} from DiscoveryDB",
            remote_guid_prefix
          );
          return;
        }
      };

    // Register remote reader & writer of topic
    // ParticipantVolatileMessageSecure, which is used for exchanging crypto
    // tokens.
    // These need to be registered before sending crypto tokens
    let local_volatile_reader_guid = self
      .local_participant_guid
      .from_prefix(EntityId::P2P_BUILTIN_PARTICIPANT_VOLATILE_SECURE_READER);
    let local_volatile_writer_guid = self
      .local_participant_guid
      .from_prefix(EntityId::P2P_BUILTIN_PARTICIPANT_VOLATILE_SECURE_WRITER);

    let remote_volatile_reader_guid = GUID::new(
      remote_guid_prefix,
      EntityId::P2P_BUILTIN_PARTICIPANT_VOLATILE_SECURE_READER,
    );
    let remote_volatile_writer_guid = GUID::new(
      remote_guid_prefix,
      EntityId::P2P_BUILTIN_PARTICIPANT_VOLATILE_SECURE_WRITER,
    );

    if let Err(e) = {
      let register_result = self
        .security_plugins
        .get_plugins()
        .register_matched_remote_reader(
          remote_volatile_reader_guid,
          local_volatile_writer_guid,
          false,
        ); // Release lock
      register_result.and_then(|()| {
        self
          .security_plugins
          .get_plugins()
          .register_matched_remote_writer(remote_volatile_writer_guid, local_volatile_reader_guid)
      })
    } {
      security_error!(
        "Failed to register remote volatile reader/writer to crypto plugin {}. Remote: {:?}",
        e,
        remote_guid_prefix
      );
      // If this registration failed, it is pointless to try to send any crypto
      // tokens. So just exit.
      return;
    }

    // Send local participant crypto tokens to remote
    // TODO: do this only if needed?
    let local_participant_crypto_tokens = self
      .security_plugins
      .get_plugins()
      // Get participant crypto tokens
      .create_local_participant_crypto_tokens(
        self.local_participant_guid.prefix,
        remote_guid_prefix,
      ); // Release lock
    let res = local_participant_crypto_tokens
      .map(|crypto_tokens| {
        self.new_volatile_message(
          GMCLASSID_SECURITY_PARTICIPANT_CRYPTO_TOKENS,
          key_exchange_writer.guid(),
          GUID::GUID_UNKNOWN, // No source endpoint, just the participant
          remote_guid_prefix,
          GUID::GUID_UNKNOWN, // No destination endpoint, just the participant
          crypto_tokens.as_ref(),
        )
      })
      // Send with writer
      .and_then(|vol_msg| {
        let opts = WriteOptionsBuilder::new()
          .to_single_reader(GUID::new(
            remote_guid_prefix,
            EntityId::P2P_BUILTIN_PARTICIPANT_VOLATILE_SECURE_READER,
          ))
          .build();
        key_exchange_writer
          .write_with_options(vol_msg, opts)
          .map_err(|write_err| {
            security_error(&format!("DataWriter write operation failed: {}", write_err))
          })
      });

    if let Err(e) = res {
      security_error!(
        "Failed to send participant crypto tokens: {}. Remote: {:?}",
        e,
        remote_guid_prefix
      );
    } else {
      info!("Sent participant crypto tokens to {:?}", remote_guid_prefix);
    }

    // Register the rest of the remote's secure built-in readers
    for (writer_eid, reader_eid, reader_endpoint) in SECURE_BUILTIN_READERS_INIT_LIST {
      if remotes_builtin_endpoints.contains(*reader_endpoint)
        && *reader_eid != EntityId::P2P_BUILTIN_PARTICIPANT_VOLATILE_SECURE_READER
      {
        let remote_reader_guid = GUID::new(remote_guid_prefix, *reader_eid);
        let local_writer_guid = self.local_participant_guid.from_prefix(*writer_eid);

        // First register remote reader
        if let Err(e) = self
          .security_plugins
          .get_plugins()
          .register_matched_remote_reader(remote_reader_guid, local_writer_guid, false)
        {
          security_error!(
            "Failed to register remote built-in reader {:?} to crypto plugin: {}",
            remote_reader_guid,
            e,
          );
          continue;
        }
        info!(
          "Registered remote reader with the crypto plugin. GUID: {:?}",
          remote_reader_guid
        );

        // Then send local writer crypto tokens to the remote reader
        let local_writer_crypto_tokens = self
          .security_plugins
          .get_plugins()
          .create_local_writer_crypto_tokens(local_writer_guid, remote_reader_guid); // Release lock
        let res = local_writer_crypto_tokens
          .map(|crypto_tokens| {
            self.new_volatile_message(
              GMCLASSID_SECURITY_DATAWRITER_CRYPTO_TOKENS,
              key_exchange_writer.guid(),
              local_writer_guid,
              remote_guid_prefix,
              remote_reader_guid,
              crypto_tokens.as_ref(),
            )
          })
          // Send with writer
          .and_then(|vol_msg| {
            let opts = WriteOptionsBuilder::new()
              .to_single_reader(GUID::new(
                remote_guid_prefix,
                EntityId::P2P_BUILTIN_PARTICIPANT_VOLATILE_SECURE_READER,
              ))
              .build();
            key_exchange_writer
              .write_with_options(vol_msg, opts)
              .map_err(|write_err| {
                security_error(&format!("DataWriter write operation failed: {}", write_err))
              })
          });

        if let Err(e) = res {
          security_error!(
            "Failed to send local writer crypto tokens: {}. Remote reader: {:?}",
            e,
            remote_reader_guid
          );
        } else {
          info!(
            "Sent local writer crypto tokens to {:?}",
            remote_reader_guid
          );
        }
      }
    }

    // Register the rest of the remote's secure built-in writers
    for (writer_eid, reader_eid, writer_endpoint) in SECURE_BUILTIN_WRITERS_INIT_LIST {
      if remotes_builtin_endpoints.contains(*writer_endpoint)
        && *writer_eid != EntityId::P2P_BUILTIN_PARTICIPANT_VOLATILE_SECURE_WRITER
      {
        let remote_writer_guid = GUID::new(remote_guid_prefix, *writer_eid);
        let local_reader_guid = self.local_participant_guid.from_prefix(*reader_eid);

        // First register remote writer
        if let Err(e) = self
          .security_plugins
          .get_plugins()
          .register_matched_remote_writer(remote_writer_guid, local_reader_guid)
        {
          security_error!(
            "Failed to register remote built-in writer {:?} to crypto plugin: {}",
            remote_writer_guid,
            e,
          );
          continue;
        } else {
          info!(
            "Registered remote writer with the crypto plugin. GUID: {:?}",
            remote_writer_guid
          );
        }

        // Then send local reader crypto tokens to the remote writer
        let local_reader_crypto_tokens = self
          .security_plugins
          .get_plugins()
          .create_local_reader_crypto_tokens(local_reader_guid, remote_writer_guid); // Release lock
        let res = local_reader_crypto_tokens
          .map(|crypto_tokens| {
            self.new_volatile_message(
              GMCLASSID_SECURITY_DATAREADER_CRYPTO_TOKENS,
              key_exchange_writer.guid(),
              local_reader_guid,
              remote_guid_prefix,
              remote_writer_guid,
              crypto_tokens.as_ref(),
            )
          })
          // Send with writer
          .and_then(|vol_msg| {
            let opts = WriteOptionsBuilder::new()
              .to_single_reader(GUID::new(
                remote_guid_prefix,
                EntityId::P2P_BUILTIN_PARTICIPANT_VOLATILE_SECURE_READER,
              ))
              .build();
            key_exchange_writer
              .write_with_options(vol_msg, opts)
              .map_err(|write_err| {
                security_error(&format!("DataWriter write operation failed: {}", write_err))
              })
          });

        if let Err(e) = res {
          security_error!(
            "Failed to send local reader crypto tokens: {}. Remote writer: {:?}",
            e,
            remote_writer_guid
          );
        } else {
          info!(
            "Sent local reader crypto tokens to {:?}",
            remote_writer_guid
          );
        }
      }
    }
  }

  fn validate_remote_participant_permissions(
    &mut self,
    remote_guid_prefix: GuidPrefix,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
  ) -> SecurityResult<()> {
    let mut sec_plugins = self.security_plugins.get_plugins();

    // Get PermissionsToken
    let permissions_token = discovery_db_read(discovery_db)
      .find_participant_proxy(remote_guid_prefix)
      .and_then(|data| data.permissions_token.clone())
      .ok_or_else(|| security_error("Could not get PermissionsToken from DiscoveryDB"))?;

    // Get AuthenticatedPeerCredentialToken
    let auth_credential_token =
      sec_plugins.get_authenticated_peer_credential_token(remote_guid_prefix)?;

    sec_plugins.validate_remote_permissions(
      self.local_participant_guid.prefix,
      remote_guid_prefix,
      &permissions_token,
      &auth_credential_token,
    )
  }

  fn resend_final_handshake_message(
    &self,
    remote_guid_prefix: GuidPrefix,
    auth_msg_writer: &no_key::DataWriter<ParticipantStatelessMessage>,
  ) {
    if let Some(stored_msg) = self.stored_authentication_messages.get(&remote_guid_prefix) {
      let _ = auth_msg_writer
        .write(stored_msg.message.clone(), None)
        .map_err(|err| {
          warn!(
            "Failed to send a final handshake message. Remote GUID prefix: {:?}. Error: {}",
            remote_guid_prefix, err
          );
        });
    } else {
      warn!(
        "Did not find the final handshake request to send. Remote guid prefix: {:?}",
        remote_guid_prefix
      );
    }
  }

  // Check if a ParticipantStatelessMessage is meant for the local participant.
  // See section 7.4.3.4 of the security spec.
  fn is_stateless_msg_for_local_participant(&self, message: &ParticipantStatelessMessage) -> bool {
    let destination_participant_guid = message.generic.destination_participant_guid;
    destination_participant_guid == self.local_participant_guid
    // Accept also if destination guid == GUID_UNKNOWN?
  }

  // Check is the message related to our unanswered message
  fn check_is_stateless_msg_related_to_our_msg(
    &self,
    message: &ParticipantStatelessMessage,
    sender_guid_prefix: GuidPrefix,
  ) -> bool {
    // Get the message sent by us
    let message_sent_by_us = match self.stored_authentication_messages.get(&sender_guid_prefix) {
      Some(msg) => &msg.message,
      None => {
        debug!(
          "Did not find an unanswered message for guid prefix {:?}",
          sender_guid_prefix
        );
        return false;
      }
    };

    message.generic.related_message_identity == message_sent_by_us.generic.message_identity
  }

  fn get_handshake_state(&self, remote_guid_prefix: &GuidPrefix) -> Option<DiscHandshakeState> {
    self.handshake_states.get(remote_guid_prefix).copied()
  }

  fn update_handshake_state(&mut self, remote_guid_prefix: GuidPrefix, state: DiscHandshakeState) {
    self.handshake_states.insert(remote_guid_prefix, state);
  }

  fn get_serialized_local_participant_data(
    &self,
    discovery_db: &Arc<RwLock<DiscoveryDB>>,
  ) -> SecurityResult<Vec<u8>> {
    let my_ser_data = discovery_db_read(discovery_db)
      .find_participant_proxy(self.local_participant_guid.prefix)
      .expect("My own participant data disappeared from DiscoveryDB")
      .to_pl_cdr_bytes(RepresentationIdentifier::PL_CDR_BE)
      .map_err(|e| security_error!("Serializing participant data failed: {e}"))?;

    Ok(my_ser_data.to_vec())
  }

  // Create a message for the DCPSParticipantStatelessMessage builtin Topic
  fn new_stateless_message(
    &mut self,
    message_class_id: &str,
    destination_guid_prefix: GuidPrefix,
    related_message_opt: Option<&ParticipantStatelessMessage>,
    handshake_token: HandshakeMessageToken,
  ) -> ParticipantStatelessMessage {
    let generic_message = self.generic_message_helper.new_message(
      message_class_id,
      self.local_participant_guid, // Writer guid for message identity
      GUID::GUID_UNKNOWN,          // Do not specify source endpoint guid
      related_message_opt.map(|msg| &msg.generic),
      destination_guid_prefix,
      GUID::GUID_UNKNOWN, // Do not specify destination endpoint guid
      vec![handshake_token.data_holder],
    );

    ParticipantStatelessMessage::from(generic_message)
  }

  // Create a message for the DCPSParticipantVolatileMessageSecure builtin Topic
  fn new_volatile_message(
    &mut self,
    message_class_id: &str,
    volatile_writer_guid: GUID,
    source_endpoint_guid: GUID,
    destination_guid_prefix: GuidPrefix,
    destination_endpoint_guid: GUID,
    crypto_tokens: &[CryptoToken],
  ) -> ParticipantVolatileMessageSecure {
    let generic_message = self.generic_message_helper.new_message(
      message_class_id,
      volatile_writer_guid,
      source_endpoint_guid,
      None, // No related message
      destination_guid_prefix,
      destination_endpoint_guid,
      crypto_tokens
        .iter()
        .map(|token| token.data_holder.clone())
        .collect(),
    );

    ParticipantVolatileMessageSecure::from(generic_message)
  }
}

fn send_discovery_notification(
  discovery_updated_sender: &mio_channel::SyncSender<DiscoveryNotificationType>,
  dntype: DiscoveryNotificationType,
) {
  match discovery_updated_sender.send(dntype) {
    Ok(_) => (),
    Err(e) => error!("Failed to send DiscoveryNotification {e:?}"),
  }
}

fn get_handshake_token_from_stateless_message(
  message: &ParticipantStatelessMessage,
) -> Option<HandshakeMessageToken> {
  let source_guid_prefix = message.generic.source_guid_prefix();
  let message_data = &message.generic.message_data;

  // We expect the message to contain only one data holder
  if message.generic.message_data.len() > 1 {
    warn!(
      "ParticipantStatelessMessage for handshake contains more than one data holder. Using only \
       the first one. Source guid prefix: {:?}",
      source_guid_prefix
    );
  }
  message_data
    .get(0)
    .map(|data_holder| HandshakeMessageToken::from(data_holder.clone()))
}

// A helper to construct ParticipantGenericMessages. Takes care of
// sequence numbering the messages
struct ParticipantGenericMessageHelper {
  next_seqnum: SequenceNumber,
}

impl ParticipantGenericMessageHelper {
  pub fn new() -> Self {
    Self {
      next_seqnum: SequenceNumber::new(1),
    }
  }

  fn get_next_seqnum(&mut self) -> SequenceNumber {
    let next = self.next_seqnum;
    // Increment for next get
    self.next_seqnum = self.next_seqnum + SequenceNumber::new(1);
    next
  }

  #[allow(clippy::too_many_arguments)]
  pub fn new_message(
    &mut self,
    message_class_id: &str,
    msg_identity_source_guid: GUID,
    source_endpoint_guid: GUID,
    related_message_opt: Option<&ParticipantGenericMessage>,
    destination_guid_prefix: GuidPrefix,
    destination_endpoint_guid: GUID,
    data_holders: Vec<DataHolder>,
  ) -> ParticipantGenericMessage {
    // See Sections 7.4.3 (ParticipantStatelessMessage) & 7.4.4
    // (ParticipantVolatileMessageSecure) of the Security specification

    let message_identity = rpc::SampleIdentity {
      writer_guid: msg_identity_source_guid,
      sequence_number: self.get_next_seqnum(),
    };

    let related_message_identity = if let Some(msg) = related_message_opt {
      msg.message_identity
    } else {
      rpc::SampleIdentity {
        writer_guid: GUID::GUID_UNKNOWN,
        sequence_number: SequenceNumber::zero(),
      }
    };

    // Make sure destination GUID has correct EntityId
    let destination_participant_guid = GUID::new(destination_guid_prefix, EntityId::PARTICIPANT);

    ParticipantGenericMessage {
      message_identity,
      related_message_identity,
      destination_participant_guid,
      destination_endpoint_guid,
      source_endpoint_guid,
      message_class_id: message_class_id.to_string(),
      message_data: data_holders,
    }
  }
}
