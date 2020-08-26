pub fn spdp_participant_data_raw() -> Vec<u8> {
  const data: [u8; 204] = [
    // Offset 0x00000000 to 0x00000203
    0x52, 0x54, 0x50, 0x53, 0x02, 0x03, 0x01, 0x0f, 0x01, 0x0f, 0x99, 0x06, 0x78, 0x34, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x09, 0x01, 0x08, 0x00, 0x0e, 0x15, 0xf3, 0x5e, 0x00, 0x28, 0x74, 0xd2,
    0x15, 0x05, 0xa8, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x01, 0x00, 0xc7, 0x00, 0x01, 0x00, 0xc2,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x15, 0x00, 0x04, 0x00,
    0x02, 0x03, 0x00, 0x00, 0x16, 0x00, 0x04, 0x00, 0x01, 0x0f, 0x00, 0x00, 0x50, 0x00, 0x10, 0x00,
    0x01, 0x0f, 0x99, 0x06, 0x78, 0x34, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0xc1,
    0x32, 0x00, 0x18, 0x00, 0x01, 0x00, 0x00, 0x00, 0xf4, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0a, 0x50, 0x8e, 0x68, 0x31, 0x00, 0x18, 0x00,
    0x01, 0x00, 0x00, 0x00, 0xf5, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x0a, 0x50, 0x8e, 0x68, 0x02, 0x00, 0x08, 0x00, 0x14, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x58, 0x00, 0x04, 0x00, 0x3f, 0x0c, 0x3f, 0x0c, 0x62, 0x00, 0x18, 0x00,
    0x14, 0x00, 0x00, 0x00, 0x66, 0x61, 0x73, 0x74, 0x72, 0x74, 0x70, 0x73, 0x50, 0x61, 0x72, 0x74,
    0x69, 0x63, 0x69, 0x70, 0x61, 0x6e, 0x74, 0x00, 0x01, 0x00, 0x00, 0x00,
  ];

  data.to_vec()
}

pub fn spdp_subscription_data_raw() -> Vec<u8> {
  const DATA: [u8; 248] = [
    // Offset 0x00000000 to 0x00000247
    0x52, 0x54, 0x50, 0x53, 0x02, 0x04, 0x01, 0x03, 0x01, 0x03, 0x00, 0x0c, 0x29, 0x2d, 0x31, 0xa2,
    0x28, 0x20, 0x02, 0x08, 0x09, 0x01, 0x08, 0x00, 0x17, 0x15, 0xf3, 0x5e, 0x35, 0x07, 0x08, 0xc2,
    0x15, 0x05, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0xc2,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x05, 0x00, 0x0c, 0x00,
    0x07, 0x00, 0x00, 0x00, 0x53, 0x71, 0x75, 0x61, 0x72, 0x65, 0x00, 0x00, 0x07, 0x00, 0x10, 0x00,
    0x0a, 0x00, 0x00, 0x00, 0x53, 0x68, 0x61, 0x70, 0x65, 0x54, 0x79, 0x70, 0x65, 0x00, 0x00, 0x00,
    0x1a, 0x00, 0x0c, 0x00, 0x01, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0x7f, 0xff, 0xff, 0xff, 0x7f,
    0x5a, 0x00, 0x10, 0x00, 0x01, 0x03, 0x00, 0x0c, 0x29, 0x2d, 0x31, 0xa2, 0x28, 0x20, 0x02, 0x08,
    0x00, 0x00, 0x00, 0x07, 0x30, 0x00, 0x18, 0x00, 0x01, 0x00, 0x00, 0x00, 0xe9, 0x1c, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xef, 0xff, 0x00, 0x02,
    0x2f, 0x00, 0x18, 0x00, 0x01, 0x00, 0x00, 0x00, 0xa6, 0x96, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0a, 0x50, 0x8e, 0xc9, 0x2f, 0x00, 0x18, 0x00,
    0x01, 0x00, 0x00, 0x00, 0xa6, 0x96, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xc0, 0xa8, 0x45, 0x14, 0x2f, 0x00, 0x18, 0x00, 0x01, 0x00, 0x00, 0x00,
    0xa6, 0x96, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xac, 0x11, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00,
  ];
  DATA.to_vec()
}

use crate::{
  serialization::{Message, cdrDeserializer},
  discovery::{
    content_filter_property::ContentFilterProperty,
    data_types::{
      topic_data::{
        SubscriptionBuiltinTopicData, ReaderProxy, WriterProxy, PublicationBuiltinTopicData,
        TopicBuiltinTopicData,
      },
      spdp_participant_data::SPDPDiscoveredParticipantData,
    },
  },
  submessages::EntitySubmessage,
  structure::{locator::Locator, guid::GUID, duration::Duration},
  dds::qos::policy::{
    Deadline, Durability, LatencyBudget, Liveliness, LivelinessKind, Reliability, Ownership,
    DestinationOrder, TimeBasedFilter, Presentation, PresentationAccessScope, Lifespan, History,
    ResourceLimits,
  },
};
use speedy::{Readable, Endianness};
use std::{net::SocketAddr, time::Duration as StdDuration};

pub fn spdp_participant_msg() -> Message {
  let data = spdp_participant_data_raw();

  let rtpsmsg = Message::read_from_buffer_with_ctx(Endianness::LittleEndian, &data).unwrap();
  rtpsmsg
}

pub fn spdp_subscription_msg() -> Message {
  let data = spdp_subscription_data_raw();

  let rtpsmsg = Message::read_from_buffer_with_ctx(Endianness::LittleEndian, &data).unwrap();
  rtpsmsg
}

pub fn spdp_participant_data() -> Option<SPDPDiscoveredParticipantData> {
  let data = spdp_participant_data_raw();

  let rtpsmsg = Message::read_from_buffer_with_ctx(Endianness::LittleEndian, &data).unwrap();
  let submsgs = rtpsmsg.submessages();

  for submsg in submsgs.iter() {
    match submsg.submessage.as_ref() {
      Some(v) => match v {
        EntitySubmessage::Data(d, _) => {
          let particiapant_data: SPDPDiscoveredParticipantData =
            cdrDeserializer::deserialize_from_little_endian(&d.serialized_payload.value).unwrap();

          return Some(particiapant_data);
        }
        _ => continue,
      },
      None => (),
    }
  }
  None
}

pub fn reader_proxy_data() -> Option<ReaderProxy> {
  let reader_proxy = ReaderProxy {
    remote_reader_guid: Some(GUID::new()),
    expects_inline_qos: Some(false),
    unicast_locator_list: vec![Locator::from(SocketAddr::new(
      "0.0.0.0".parse().unwrap(),
      12345,
    ))],
    multicast_locator_list: vec![Locator::from(SocketAddr::new(
      "0.0.0.0".parse().unwrap(),
      13579,
    ))],
  };

  Some(reader_proxy)
}

pub fn writer_proxy_data() -> Option<WriterProxy> {
  let writer_proxy = WriterProxy {
    remote_writer_guid: Some(GUID::new()),
    unicast_locator_list: vec![Locator::from(SocketAddr::new(
      "0.0.0.0".parse().unwrap(),
      12345,
    ))],
    multicast_locator_list: vec![Locator::from(SocketAddr::new(
      "0.0.0.0".parse().unwrap(),
      13579,
    ))],
    data_max_size_serialized: Some(24000),
  };

  Some(writer_proxy)
}

pub fn subscription_builtin_topic_data() -> Option<SubscriptionBuiltinTopicData> {
  let sub_topic_data = SubscriptionBuiltinTopicData {
    key: Some(GUID::new()),
    participant_key: Some(GUID::new()),
    topic_name: Some("some topic name".to_string()),
    type_name: Some("RandomData".to_string()),
    durability: Some(Durability::TransientLocal),
    deadline: Some(Deadline {
      period: Duration::from(StdDuration::from_secs(60)),
    }),
    latency_budget: Some(LatencyBudget {
      duration: Duration::from(StdDuration::from_secs(2 * 60)),
    }),
    liveliness: Some(Liveliness {
      kind: LivelinessKind::ManulByTopic,
      lease_duration: Duration::from(StdDuration::from_secs(3 * 60)),
    }),
    reliability: Some(Reliability::Reliable {
      max_blocking_time: Duration::from(StdDuration::from_secs(4 * 60)),
    }),
    ownership: Some(Ownership::Exclusive { strength: 234 }),
    destination_order: Some(DestinationOrder::BySourceTimeStamp),
    time_based_filter: Some(TimeBasedFilter {
      minimum_separation: Duration::from(StdDuration::from_secs(5 * 60)),
    }),
    presentation: Some(Presentation {
      access_scope: PresentationAccessScope::Topic,
      coherent_access: false,
      ordered_access: true,
    }),
    lifespan: Some(Lifespan {
      duration: Duration::from(StdDuration::from_secs(6 * 60)),
    }),
  };

  Some(sub_topic_data)
}

pub fn publication_builtin_topic_data() -> Option<PublicationBuiltinTopicData> {
  let pub_topic_data = PublicationBuiltinTopicData {
    key: Some(GUID::new()),
    participant_key: Some(GUID::new()),
    topic_name: Some("rand topic namm".to_string()),
    type_name: Some("RandomData".to_string()),
    durability: Some(Durability::Volatile),
    deadline: Some(Deadline {
      period: Duration::from(StdDuration::from_secs(30)),
    }),
    latency_budget: Some(LatencyBudget {
      duration: Duration::from(StdDuration::from_secs(2 * 30)),
    }),
    liveliness: Some(Liveliness {
      kind: LivelinessKind::ManulByTopic,
      lease_duration: Duration::from(StdDuration::from_secs(3 * 30)),
    }),
    reliability: Some(Reliability::BestEffort),
    lifespan: Some(Lifespan {
      duration: Duration::from(StdDuration::from_secs(6 * 30)),
    }),
    time_based_filter: Some(TimeBasedFilter {
      minimum_separation: Duration::from(StdDuration::from_secs(5 * 30)),
    }),
    ownership: Some(Ownership::Shared),
    destination_order: Some(DestinationOrder::ByReceptionTimestamp),
    presentation: Some(Presentation {
      access_scope: PresentationAccessScope::Instance,
      coherent_access: true,
      ordered_access: false,
    }),
  };

  Some(pub_topic_data)
}

pub fn topic_data() -> Option<TopicBuiltinTopicData> {
  let topic_data = TopicBuiltinTopicData {
    key: Some(GUID::new()),
    name: Some("SomeTopicName".to_string()),
    type_name: Some("RandomData".to_string()),
    durability: Some(Durability::Persistent),
    deadline: Some(Deadline {
      period: Duration::from(StdDuration::from_secs(45)),
    }),
    latency_budget: Some(LatencyBudget {
      duration: Duration::from(StdDuration::from_secs(2 * 45)),
    }),
    liveliness: Some(Liveliness {
      kind: LivelinessKind::ManulByTopic,
      lease_duration: Duration::from(StdDuration::from_secs(3 * 45)),
    }),
    reliability: Some(Reliability::BestEffort),
    lifespan: Some(Lifespan {
      duration: Duration::from(StdDuration::from_secs(6 * 45)),
    }),
    destination_order: Some(DestinationOrder::ByReceptionTimestamp),
    presentation: Some(Presentation {
      access_scope: PresentationAccessScope::Group,
      coherent_access: true,
      ordered_access: true,
    }),
    history: Some(History::KeepLast { depth: 25 }),
    resource_limits: Some(ResourceLimits {
      max_samples: 5,
      max_instances: 10,
      max_samples_per_instance: 15,
    }),
    ownership: Some(Ownership::Exclusive { strength: 432 }),
  };

  Some(topic_data)
}

pub fn content_filter_data() -> Option<ContentFilterProperty> {
  let content_filter = ContentFilterProperty {
    contentFilteredTopicName: "tn".to_string(),
    relatedTopicName: "rtn".to_string(),
    filterClassName: "fcn".to_string(),
    filterExpression: "fexp".to_string(),
    expressionParameters: vec!["asdf".to_string(), "fdsas".to_string()],
  };

  Some(content_filter)
}
