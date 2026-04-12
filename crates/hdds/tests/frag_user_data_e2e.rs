// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! End-to-end fragmentation for user data over UDP.
//!
//! Reproduces Test_LargeData_0 from the OMG DDS-RTPS suite:
//! a writer sends a sample with a 100 KB payload, the reader must receive it.
//! Exercises the full path: writer encode -> DATA_FRAG packets -> UDP loopback
//! -> MulticastListener classify -> router loop reassembly -> reader cache.

#![allow(clippy::uninlined_format_args)]

use hdds::{Participant, QoS, TransportMode};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, hdds::DDS)]
struct BigBlob {
    id: u32,
    data: Vec<u8>,
}

#[test]
fn test_100kb_sample_udp_roundtrip() {
    // 100KB payload — forces DATA_FRAG on the wire (HDDS fragments above 8KB).
    const PAYLOAD_SIZE: usize = 100_000;
    const DOMAIN: u32 = 77;

    let pub_part = Participant::builder("big_blob_pub")
        .domain_id(DOMAIN)
        .with_transport(TransportMode::UdpMulticast)
        .build()
        .expect("publisher participant");
    let sub_part = Participant::builder("big_blob_sub")
        .domain_id(DOMAIN)
        .with_transport(TransportMode::UdpMulticast)
        .build()
        .expect("subscriber participant");

    // Wait for SPDP + SEDP discovery.
    thread::sleep(Duration::from_millis(500));

    let writer = pub_part
        .topic::<BigBlob>("LargeDataTopic")
        .expect("writer topic")
        .writer()
        .qos(QoS::reliable().keep_all())
        .build()
        .expect("writer");

    let reader = sub_part
        .topic::<BigBlob>("LargeDataTopic")
        .expect("reader topic")
        .reader()
        .qos(QoS::reliable().keep_all())
        .build()
        .expect("reader");

    // Give both sides time to match.
    thread::sleep(Duration::from_millis(500));

    let sample = BigBlob {
        id: 42,
        data: vec![0xABu8; PAYLOAD_SIZE],
    };
    writer.write(&sample).expect("write 100KB sample");

    // Give the fragments time to travel and reassemble.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut received: Option<BigBlob> = None;
    while std::time::Instant::now() < deadline {
        match reader.take() {
            Ok(Some(msg)) => {
                received = Some(msg);
                break;
            }
            Ok(None) | Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }

    let msg = received.expect("subscriber should receive the 100KB sample");
    assert_eq!(msg.id, 42);
    assert_eq!(msg.data.len(), PAYLOAD_SIZE);
    assert!(
        msg.data.iter().all(|&b| b == 0xAB),
        "payload corrupted during reassembly"
    );
}
