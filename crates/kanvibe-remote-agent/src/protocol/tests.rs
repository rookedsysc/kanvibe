use serde_json::json;

use super::*;

fn record(value: serde_json::Value) -> HostRecord {
    record_from(value)
}

// parity: TS-lib-remoteHostProtocol-f5f4ec
#[test]
fn reassembles_split_headers_and_unicode_payloads_with_adjacent_records() {
    let expected = vec![
        record(json!({ "type": "message", "text": "한글😀" })),
        record(json!({ "type": "status", "id": "next" })),
    ];
    let bytes: Vec<u8> = expected
        .iter()
        .flat_map(|record| encode_host_record(record).unwrap())
        .collect();
    let mut decoder = HostRecordDecoder::new();

    let decoded: Vec<HostRecord> = bytes
        .iter()
        .flat_map(|byte| decoder.push(&[*byte]).unwrap())
        .collect();

    assert_eq!(decoded, expected);
}

// parity: TS-lib-remoteHostProtocol-a0fdf2
#[test]
fn rejects_oversized_lengths_before_receiving_a_payload() {
    let prefix = ((HOST_RECORD_BYTES + 1) as u32).to_be_bytes();

    let result = HostRecordDecoder::new().push(&prefix);

    assert_eq!(result, Err(HostWireError::RecordSizeExceeded));
}

// parity: TS-lib-remoteHostProtocol-09f298
#[test]
fn rejects_non_record_json_and_malformed_utf8() {
    let array = [&2u32.to_be_bytes()[..], b"[]"].concat();
    let malformed = [&1u32.to_be_bytes()[..], &[255]].concat();

    assert!(HostRecordDecoder::new().push(&array).is_err());
    assert!(HostRecordDecoder::new().push(&malformed).is_err());
}

// parity: TS-lib-remoteHostProtocol-41c239
#[test]
fn accepts_only_canonical_base64_and_enforces_byte_bounds() {
    let korean = json!(encode_host_bytes("한글".as_bytes()));
    let oversized = json!(encode_host_bytes(&vec![0; HOST_CHUNK_BYTES + 1]));

    assert_eq!(
        decode_host_bytes(Some(&korean), HOST_CHUNK_BYTES).unwrap(),
        "한글".as_bytes()
    );
    assert!(decode_host_bytes(Some(&json!("@@@=")), HOST_CHUNK_BYTES).is_err());
    assert!(decode_host_bytes(Some(&oversized), HOST_CHUNK_BYTES).is_err());
}

// parity: TS-lib-remoteHostProtocol-100275
#[test]
fn recognizes_explicit_namespaces() {
    for namespace in ["dev", "prod", "qa-fixture-42"] {
        assert!(is_valid_host_namespace(namespace), "{namespace}");
        assert_eq!(
            build_host_socket_paths(namespace, 501)
                .unwrap()
                .rpc_socket_path,
            format!("/tmp/kv-501/{namespace}/rpc-v1.sock")
        );
    }
}

// parity: TS-lib-remoteHostProtocol-771d6e
#[test]
fn rejects_path_and_unknown_namespaces() {
    let too_long = format!("qa-{}", "x".repeat(40));
    for namespace in ["../dev", "", "DEV", too_long.as_str()] {
        assert!(!is_valid_host_namespace(namespace), "{namespace}");
        assert!(build_host_socket_paths(namespace, 501).is_err());
    }
}
