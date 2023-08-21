use core::fmt;
use std::{
  collections::HashMap,
  sync::{Arc, Mutex, MutexGuard},
};

use crate::{
  messages::submessages::{
    elements::parameter_list::ParameterList,
    secure_postfix::SecurePostfix,
    secure_prefix::SecurePrefix,
    submessage::{ReaderSubmessage, WriterSubmessage},
  },
  rtps::{Message, Submessage},
  security_error,
  structure::guid::GuidPrefix,
  QosPolicies, GUID,
};
use super::{
  access_control::*,
  authentication::*,
  cryptographic::{
    DatareaderCryptoHandle, DatawriterCryptoHandle, EncodedSubmessage, EndpointCryptoHandle,
    ParticipantCryptoHandle, SecureSubmessageKind,
  },
  types::*,
  AccessControl, Cryptographic,
};

pub(crate) struct SecurityPlugins {
  auth: Box<dyn Authentication>,
  access: Box<dyn AccessControl>,
  crypto: Box<dyn Cryptographic>,

  identity_handle_cache_: HashMap<GuidPrefix, IdentityHandle>,
  permissions_handle_cache_: HashMap<GuidPrefix, PermissionsHandle>,
  handshake_handle_cache_: HashMap<GuidPrefix, HandshakeHandle>,

  participant_crypto_handle_cache_: HashMap<GuidPrefix, ParticipantCryptoHandle>,
  local_endpoint_crypto_handle_cache_: HashMap<GUID, EndpointCryptoHandle>,
  remote_endpoint_crypto_handle_cache_: HashMap<(GUID, GUID), EndpointCryptoHandle>,

  test_disable_crypto_transform_: bool, /* TODO: Disables the crypto transform interface, remove
                                         * after testing */
}

impl SecurityPlugins {
  pub fn new(
    auth: Box<impl Authentication + 'static>,
    access: Box<impl AccessControl + 'static>,
    crypto: Box<impl Cryptographic + 'static>,
  ) -> Self {
    Self {
      auth,
      access,
      crypto,
      identity_handle_cache_: HashMap::new(),
      permissions_handle_cache_: HashMap::new(),
      handshake_handle_cache_: HashMap::new(),
      participant_crypto_handle_cache_: HashMap::new(),
      local_endpoint_crypto_handle_cache_: HashMap::new(),
      remote_endpoint_crypto_handle_cache_: HashMap::new(),

      test_disable_crypto_transform_: true, // TODO Remove after testing
    }
  }

  fn get_identity_handle(&self, guidp: &GuidPrefix) -> SecurityResult<IdentityHandle> {
    self
      .identity_handle_cache_
      .get(guidp)
      .ok_or_else(|| {
        security_error!(
          "Could not find an IdentityHandle for the GUID prefix {:?}",
          guidp
        )
      })
      .copied()
  }

  fn get_permissions_handle(&self, guidp: &GuidPrefix) -> SecurityResult<PermissionsHandle> {
    self
      .permissions_handle_cache_
      .get(guidp)
      .ok_or_else(|| {
        security_error!(
          "Could not find a PermissionsHandle for the GUID prefix {:?}",
          guidp
        )
      })
      .copied()
  }

  fn get_participant_crypto_handle(
    &self,
    guid_prefix: &GuidPrefix,
  ) -> SecurityResult<ParticipantCryptoHandle> {
    self
      .participant_crypto_handle_cache_
      .get(guid_prefix)
      .ok_or_else(|| {
        security_error!(
          "Could not find a ParticipantCryptoHandle for the GuidPrefix {:?}",
          guid_prefix
        )
      })
      .copied()
  }

  fn get_local_endpoint_crypto_handle(&self, guid: &GUID) -> SecurityResult<EndpointCryptoHandle> {
    self
      .local_endpoint_crypto_handle_cache_
      .get(guid)
      .ok_or_else(|| {
        security_error!(
          "Could not find a local EndpointCryptoHandle for the GUID {:?}",
          guid
        )
      })
      .copied()
  }

  /// The `local_proxy_guid_pair` should be `&(local_endpoint_guid,
  /// proxy_guid)`.
  fn get_remote_endpoint_crypto_handle(
    &self,
    (local_endpoint_guid, proxy_guid): (&GUID, &GUID),
  ) -> SecurityResult<ParticipantCryptoHandle> {
    let local_and_proxy_guid_pair = (*local_endpoint_guid, *proxy_guid);
    self
      .remote_endpoint_crypto_handle_cache_
      .get(&local_and_proxy_guid_pair)
      .ok_or_else(|| {
        security_error!(
          "Could not find a remote EndpointCryptoHandle for the (local_endpoint_guid, proxy_guid) \
           pair {:?}",
          local_and_proxy_guid_pair
        )
      })
      .copied()
  }
}

/// Interface for using the Authentication plugin
impl SecurityPlugins {
  pub fn validate_local_identity(
    &mut self,
    domain_id: u16,
    participant_qos: &QosPolicies,
    candidate_participant_guid: GUID,
  ) -> SecurityResult<GUID> {
    let (outcome, identity_handle, sec_guid) =
      self
        .auth
        .validate_local_identity(domain_id, participant_qos, candidate_participant_guid)?;

    if let ValidationOutcome::Ok = outcome {
      // Everything OK, store handle and return GUID
      self
        .identity_handle_cache_
        .insert(sec_guid.prefix, identity_handle);
      Ok(sec_guid)
    } else {
      // If the builtin authentication does not fail, it should produce only OK
      // outcome. If some other outcome was produced, return an error
      Err(security_error!(
        "Validating local identity produced an unexpected outcome"
      ))
    }
  }

  pub fn get_identity_token(&self, participant_guidp: GuidPrefix) -> SecurityResult<IdentityToken> {
    let identity_handle = self.get_identity_handle(&participant_guidp)?;
    self.auth.get_identity_token(identity_handle)
  }

  pub fn get_identity_status_token(
    &self,
    participant_guidp: GuidPrefix,
  ) -> SecurityResult<IdentityStatusToken> {
    let identity_handle = self.get_identity_handle(&participant_guidp)?;
    self.auth.get_identity_status_token(identity_handle)
  }

  pub fn set_permissions_credential_and_token(
    &self,
    participant_guidp: GuidPrefix,
    permissions_credential_token: PermissionsCredentialToken,
    permissions_token: PermissionsToken,
  ) -> SecurityResult<()> {
    let handle = self.get_identity_handle(&participant_guidp)?;
    self.auth.set_permissions_credential_and_token(
      handle,
      permissions_credential_token,
      permissions_token,
    )
  }

  pub fn validate_remote_identity(
    &mut self,
    local_participant_guidp: GuidPrefix,
    remote_identity_token: IdentityToken,
    remote_participant_guidp: GuidPrefix,
    remote_auth_request_token: Option<AuthRequestMessageToken>,
  ) -> SecurityResult<(ValidationOutcome, Option<AuthRequestMessageToken>)> {
    let local_identity_handle = self.get_identity_handle(&local_participant_guidp)?;

    let (outcome, remote_id_handle, auth_req_token_opt) = self.auth.validate_remote_identity(
      remote_auth_request_token,
      local_identity_handle,
      remote_identity_token,
      remote_participant_guidp,
    )?;

    // Add remote identity handle to cache
    self
      .identity_handle_cache_
      .insert(remote_participant_guidp, remote_id_handle);

    Ok((outcome, auth_req_token_opt))
  }

  pub fn begin_handshake_request(
    &mut self,
    initiator_guidp: GuidPrefix,
    replier_guidp: GuidPrefix,
    serialized_local_participant_data: Vec<u8>,
  ) -> SecurityResult<HandshakeMessageToken> {
    let initiator_identity_handle = self.get_identity_handle(&initiator_guidp)?;
    let replier_identity_handle = self.get_identity_handle(&replier_guidp)?;

    let (outcome, handshake_handle, handshake_token) = self.auth.begin_handshake_request(
      initiator_identity_handle,
      replier_identity_handle,
      serialized_local_participant_data,
    )?;

    if let ValidationOutcome::PendingHandshakeMessage = outcome {
      // This is the only expected OK outcome from builtin plugin
      // Store handshake handle and return token
      self
        .handshake_handle_cache_
        .insert(initiator_guidp, handshake_handle);
      Ok(handshake_token)
    } else {
      Err(security_error!(
        "Unexptected validation outcome from begin_handshake_request. Outcome: {:?}",
        outcome
      ))
    }
  }
}

/// Interface for using the Access control plugin
impl SecurityPlugins {
  pub fn validate_local_permissions(
    &mut self,
    domain_id: u16,
    participant_guidp: GuidPrefix,
    participant_qos: &QosPolicies,
  ) -> SecurityResult<()> {
    let identity_handle = self.get_identity_handle(&participant_guidp)?;
    let permissions_handle = self.access.validate_local_permissions(
      &*self.auth,
      identity_handle,
      domain_id,
      participant_qos,
    )?;
    self
      .permissions_handle_cache_
      .insert(participant_guidp, permissions_handle);
    Ok(())
  }

  pub fn check_create_participant(
    &self,
    domain_id: u16,
    participant_guidp: GuidPrefix,
    qos: &QosPolicies,
  ) -> SecurityResult<()> {
    let handle = self.get_permissions_handle(&participant_guidp)?;
    self.access.check_create_participant(handle, domain_id, qos)
  }

  pub fn get_permissions_token(
    &self,
    participant_guidp: GuidPrefix,
  ) -> SecurityResult<PermissionsToken> {
    let handle: PermissionsHandle = self.get_permissions_handle(&participant_guidp)?;
    self.access.get_permissions_token(handle)
  }

  pub fn get_permissions_credential_token(
    &self,
    participant_guidp: GuidPrefix,
  ) -> SecurityResult<PermissionsCredentialToken> {
    let handle: PermissionsHandle = self.get_permissions_handle(&participant_guidp)?;
    self.access.get_permissions_credential_token(handle)
  }

  pub fn get_participant_sec_attributes(
    &self,
    participant_guidp: GuidPrefix,
  ) -> SecurityResult<ParticipantSecurityAttributes> {
    let handle: PermissionsHandle = self.get_permissions_handle(&participant_guidp)?;
    self.access.get_participant_sec_attributes(handle)
  }
}

/// Interface for using the CryptoTransform of the Cryptographic plugin
impl SecurityPlugins {
  pub fn encode_serialized_payload(
    &self,
    serialized_payload: Vec<u8>,
    sending_datawriter_guid: &GUID,
  ) -> SecurityResult<(Vec<u8>, ParameterList)> {
    // TODO remove after testing, skips encoding
    if self.test_disable_crypto_transform_ {
      return Ok((serialized_payload, ParameterList::new()));
    }

    self.crypto.encode_serialized_payload(
      serialized_payload,
      self.get_local_endpoint_crypto_handle(sending_datawriter_guid)?,
    )
  }

  pub fn encode_datawriter_submessage(
    &self,
    plain_submessage: Submessage,
    source_guid: &GUID,
    destination_guid_list: &[GUID],
  ) -> SecurityResult<EncodedSubmessage> {
    // TODO remove after testing, skips encoding
    if self.test_disable_crypto_transform_ {
      return Ok(EncodedSubmessage::Unencoded(plain_submessage));
    }

    // Convert the destination GUIDs to crypto handles
    let mut receiving_datareader_crypto_list: Vec<DatareaderCryptoHandle> =
      SecurityResult::from_iter(destination_guid_list.iter().map(|destination_guid| {
        self.get_remote_endpoint_crypto_handle((source_guid, destination_guid))
      }))?;
    // Remove duplicates
    receiving_datareader_crypto_list.sort();
    receiving_datareader_crypto_list.dedup();

    self.crypto.encode_datawriter_submessage(
      plain_submessage,
      self.get_local_endpoint_crypto_handle(source_guid)?,
      receiving_datareader_crypto_list,
    )
  }

  pub fn encode_datareader_submessage(
    &self,
    plain_submessage: Submessage,
    source_guid: &GUID,
    destination_guid_list: &[GUID],
  ) -> SecurityResult<EncodedSubmessage> {
    // TODO remove after testing, skips encoding
    if self.test_disable_crypto_transform_ {
      return Ok(EncodedSubmessage::Unencoded(plain_submessage));
    }

    // Convert the destination GUIDs to crypto handles
    let mut receiving_datawriter_crypto_list: Vec<DatawriterCryptoHandle> =
      SecurityResult::from_iter(destination_guid_list.iter().map(|destination_guid| {
        self.get_remote_endpoint_crypto_handle((source_guid, destination_guid))
      }))?;
    // Remove duplicates
    receiving_datawriter_crypto_list.sort();
    receiving_datawriter_crypto_list.dedup();

    self.crypto.encode_datareader_submessage(
      plain_submessage,
      self.get_local_endpoint_crypto_handle(source_guid)?,
      receiving_datawriter_crypto_list,
    )
  }

  pub fn encode_message(
    &self,
    plain_message: Message,
    source_guid_prefix: &GuidPrefix,
    destination_guid_prefix_list: &[GuidPrefix],
  ) -> SecurityResult<Message> {
    // TODO remove after testing, skips encoding
    if self.test_disable_crypto_transform_ {
      return Ok(plain_message);
    }

    // Convert the destination GUID prefixes to crypto handles
    let mut receiving_datawriter_crypto_list: Vec<DatawriterCryptoHandle> =
      SecurityResult::from_iter(destination_guid_prefix_list.iter().map(
        |destination_guid_prefix| self.get_participant_crypto_handle(destination_guid_prefix),
      ))?;
    // Remove duplicates
    receiving_datawriter_crypto_list.sort();
    receiving_datawriter_crypto_list.dedup();

    self.crypto.encode_rtps_message(
      plain_message,
      self.get_participant_crypto_handle(source_guid_prefix)?,
      receiving_datawriter_crypto_list,
    )
  }

  pub fn decode_rtps_message(
    &self,
    encoded_message: Message,
    source_guid_prefix: &GuidPrefix,
    destination_guid_prefix: &GuidPrefix,
  ) -> SecurityResult<Message> {
    self.crypto.decode_rtps_message(
      encoded_message,
      self.get_participant_crypto_handle(destination_guid_prefix)?,
      self.get_participant_crypto_handle(source_guid_prefix)?,
    )
  }

  pub fn preprocess_secure_submessage(
    &self,
    secure_prefix: &SecurePrefix,
    source_guid_prefix: &GuidPrefix,
    destination_guid_prefix: &GuidPrefix,
  ) -> SecurityResult<SecureSubmessageKind> {
    self.crypto.preprocess_secure_submessage(
      secure_prefix,
      self.get_participant_crypto_handle(destination_guid_prefix)?,
      self.get_participant_crypto_handle(source_guid_prefix)?,
    )
  }

  pub fn decode_datawriter_submessage(
    &self,
    encoded_rtps_submessage: (SecurePrefix, Submessage, SecurePostfix),
    receiving_datareader_crypto: DatareaderCryptoHandle,
    sending_datawriter_crypto: DatawriterCryptoHandle,
  ) -> SecurityResult<WriterSubmessage> {
    self.crypto.decode_datawriter_submessage(
      encoded_rtps_submessage,
      receiving_datareader_crypto,
      sending_datawriter_crypto,
    )
  }

  pub fn decode_datareader_submessage(
    &self,
    encoded_rtps_submessage: (SecurePrefix, Submessage, SecurePostfix),
    receiving_datawriter_crypto: DatawriterCryptoHandle,
    sending_datareader_crypto: DatareaderCryptoHandle,
  ) -> SecurityResult<ReaderSubmessage> {
    self.crypto.decode_datareader_submessage(
      encoded_rtps_submessage,
      receiving_datawriter_crypto,
      sending_datareader_crypto,
    )
  }

  pub fn decode_serialized_payload(
    &self,
    encoded_payload: Vec<u8>,
    inline_qos: ParameterList,
    source_guid: &GUID,
    destination_guid: &GUID,
  ) -> SecurityResult<Vec<u8>> {
    // TODO remove after testing, skips decoding
    if self.test_disable_crypto_transform_ {
      return Ok(encoded_payload);
    }

    self.crypto.decode_serialized_payload(
      encoded_payload,
      inline_qos,
      self.get_local_endpoint_crypto_handle(destination_guid)?,
      self.get_remote_endpoint_crypto_handle((destination_guid, source_guid))?,
    )
  }
}

#[derive(Clone)]
pub(crate) struct SecurityPluginsHandle {
  inner: Arc<Mutex<SecurityPlugins>>,
}

impl SecurityPluginsHandle {
  pub(crate) fn new(s: SecurityPlugins) -> Self {
    Self {
      inner: Arc::new(Mutex::new(s)),
    }
  }

  pub(crate) fn get_mutex_guard(
    security_plugins: Option<&SecurityPluginsHandle>,
  ) -> SecurityResult<Option<MutexGuard<SecurityPlugins>>> {
    security_plugins
      // Get a mutex guard
      .map(|security_plugins_handle| security_plugins_handle.lock())
      // Dig out the error
      .transpose()
      .map_err(|e| {
        security_error!("SecurityPluginHandle poisoned! {e:?}")
        // TODO: Send signal to exit RTPS thread, as there is no way to
        // recover.
      })
  }
}

impl fmt::Debug for SecurityPluginsHandle {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("SecurityPluginsHandle")
  }
}

impl std::ops::Deref for SecurityPluginsHandle {
  type Target = Mutex<SecurityPlugins>;
  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}
