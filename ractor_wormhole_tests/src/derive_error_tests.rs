use std::sync::atomic::{AtomicU64, Ordering};

use ractor::{Actor, ActorProcessingErr, ActorRef, concurrency::Duration};
use ractor_wormhole::portal::PortalActorMessage;
use ractor_wormhole::transmaterialization::{ContextTransmaterializable, TransmaterializationContext};

use crate::derive_tests::{MixedEnum, StructEnum};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Minimal dummy actor that satisfies the PortalActorMessage type requirement.
/// Used for testing serialization of simple types that don't actually use the context.
struct DummyPortalActor;

impl Actor for DummyPortalActor {
    type Msg = PortalActorMessage;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        _message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        // Never actually called in these tests
        Ok(())
    }
}

async fn create_test_context() -> TransmaterializationContext {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);

    let (actor_ref, _handle) = Actor::spawn(
        Some(format!("dummy-portal-{}", id)),
        DummyPortalActor,
        (),
    )
    .await
    .expect("failed to spawn dummy actor");

    TransmaterializationContext {
        connection: actor_ref,
        default_rpc_port_timeout: Duration::from_secs(5),
    }
}

#[tokio::test]
async fn test_unit_variant_rejects_non_empty_payload() {
    let ctx = create_test_context().await;

    // Serialize a unit variant
    let unit_variant = MixedEnum::Unit;
    let serialized = unit_variant.immaterialize(&ctx).await.unwrap();

    // The format is: [variant_name_len: u64][variant_name: bytes][payload_len: u64][payload: bytes]
    // For Unit variant, payload should be empty

    // Tamper with the payload: insert extra bytes
    let mut tampered = serialized.clone();
    // Find where payload_len is (after variant name)
    // variant_name = "Unit" (4 bytes), so offset is 8 + 4 = 12
    // payload_len is at offset 12, and we need to change it and add bytes
    let payload_len_offset = 8 + 4; // 8 bytes for name len + 4 bytes for "Unit"

    // Change payload length from 0 to 4
    tampered[payload_len_offset..payload_len_offset + 8].copy_from_slice(&4u64.to_le_bytes());
    // Add 4 garbage bytes
    tampered.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

    let result = MixedEnum::rematerialize(&ctx, &tampered).await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Unit variant should have empty payload"),
        "Expected unit variant error, got: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_tuple_variant_rejects_extra_payload() {
    let ctx = create_test_context().await;

    // Serialize a tuple variant
    let tuple_variant = MixedEnum::Tuple(42, "hello".to_string());
    let serialized = tuple_variant.immaterialize(&ctx).await.unwrap();

    // Add extra bytes to the payload
    let mut tampered = serialized.clone();
    // The payload length is stored after the variant name
    // variant_name = "Tuple" (5 bytes)
    let name_len_bytes = &serialized[0..8];
    let name_len = u64::from_le_bytes(name_len_bytes.try_into().unwrap()) as usize;
    let payload_len_offset = 8 + name_len;

    // Read current payload length
    let payload_len = u64::from_le_bytes(
        serialized[payload_len_offset..payload_len_offset + 8]
            .try_into()
            .unwrap(),
    ) as usize;

    // Increase payload length by 4 and add garbage
    let new_payload_len = (payload_len + 4) as u64;
    tampered[payload_len_offset..payload_len_offset + 8].copy_from_slice(&new_payload_len.to_le_bytes());
    tampered.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

    let result = MixedEnum::rematerialize(&ctx, &tampered).await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("payload not fully consumed"),
        "Expected payload consumption error, got: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_struct_variant_rejects_extra_payload() {
    let ctx = create_test_context().await;

    // Serialize a struct variant
    let struct_variant = MixedEnum::Struct {
        a: 123,
        b: "world".to_string(),
    };
    let serialized = struct_variant.immaterialize(&ctx).await.unwrap();

    // Add extra bytes to the payload
    let mut tampered = serialized.clone();
    let name_len_bytes = &serialized[0..8];
    let name_len = u64::from_le_bytes(name_len_bytes.try_into().unwrap()) as usize;
    let payload_len_offset = 8 + name_len;

    let payload_len = u64::from_le_bytes(
        serialized[payload_len_offset..payload_len_offset + 8]
            .try_into()
            .unwrap(),
    ) as usize;

    let new_payload_len = (payload_len + 4) as u64;
    tampered[payload_len_offset..payload_len_offset + 8].copy_from_slice(&new_payload_len.to_le_bytes());
    tampered.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

    let result = MixedEnum::rematerialize(&ctx, &tampered).await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("payload not fully consumed"),
        "Expected payload consumption error, got: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_struct_enum_variant_rejects_extra_payload() {
    let ctx = create_test_context().await;

    // Test StructEnum which only has struct variants
    let variant = StructEnum::X { a: 42 };
    let serialized = variant.immaterialize(&ctx).await.unwrap();

    let mut tampered = serialized.clone();
    let name_len = u64::from_le_bytes(serialized[0..8].try_into().unwrap()) as usize;
    let payload_len_offset = 8 + name_len;

    let payload_len = u64::from_le_bytes(
        serialized[payload_len_offset..payload_len_offset + 8]
            .try_into()
            .unwrap(),
    ) as usize;

    let new_payload_len = (payload_len + 4) as u64;
    tampered[payload_len_offset..payload_len_offset + 8].copy_from_slice(&new_payload_len.to_le_bytes());
    tampered.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

    let result = StructEnum::rematerialize(&ctx, &tampered).await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("payload not fully consumed"),
        "Expected payload consumption error, got: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_truncated_data_returns_error() {
    let ctx = create_test_context().await;

    // Serialize a struct variant
    let variant = MixedEnum::Struct {
        a: 123,
        b: "hello".to_string(),
    };
    let serialized = variant.immaterialize(&ctx).await.unwrap();

    // Truncate the data at various points
    for truncate_at in [0, 4, 8, 12, 16, serialized.len() / 2, serialized.len() - 1] {
        if truncate_at >= serialized.len() {
            continue;
        }
        let truncated = &serialized[..truncate_at];
        let result = MixedEnum::rematerialize(&ctx, truncated).await;
        assert!(
            result.is_err(),
            "Expected error for data truncated at {} bytes, but got Ok",
            truncate_at
        );
    }
}

#[tokio::test]
async fn test_roundtrip_still_works() {
    let ctx = create_test_context().await;

    // Verify that valid data still deserializes correctly
    let variants = vec![
        MixedEnum::Unit,
        MixedEnum::Tuple(42, "hello".to_string()),
        MixedEnum::Struct {
            a: 123,
            b: "world".to_string(),
        },
    ];

    for variant in variants {
        let serialized = variant.clone().immaterialize(&ctx).await.unwrap();
        let deserialized = MixedEnum::rematerialize(&ctx, &serialized).await.unwrap();
        assert_eq!(variant, deserialized);
    }
}
