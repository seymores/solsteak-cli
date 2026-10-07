//! Run using the isolated manifest in docs/decoder-reference.md.
use base64::{Engine, engine::general_purpose::STANDARD};
use solana_pubkey::Pubkey;
use solana_stake_interface::{
    stake_flags::StakeFlags,
    state::{Authorized, Delegation, Lockup, Meta, Stake, StakeStateV2},
};

#[allow(deprecated)]
fn main() {
    let meta = Meta {
        rent_exempt_reserve: 2_282_880,
        authorized: Authorized {
            staker: Pubkey::new_from_array([1; 32]),
            withdrawer: Pubkey::new_from_array([2; 32]),
        },
        lockup: Lockup {
            unix_timestamp: i64::MIN,
            epoch: u64::MAX,
            custodian: Pubkey::new_from_array([3; 32]),
        },
    };
    let stake = Stake {
        delegation: Delegation {
            voter_pubkey: Pubkey::new_from_array([4; 32]),
            stake: u64::MAX - 2_282_880,
            activation_epoch: u64::MAX,
            deactivation_epoch: u64::MAX,
            _reserved: [0; 8],
        },
        credits_observed: 9_007_199_254_740_993,
    };
    for (name, state) in [
        ("initialized", StakeStateV2::Initialized(meta)),
        (
            "delegated",
            StakeStateV2::Stake(
                meta,
                stake,
                StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED,
            ),
        ),
    ] {
        let mut bytes = bincode::serialize(&state).unwrap();
        bytes.resize(StakeStateV2::size_of(), 0);
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(format!("tests/fixtures/stake/{name}.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(fixture["data"][0].as_str().unwrap(), STANDARD.encode(bytes));
        println!("{name}: matches canonical StakeStateV2 serialization");
    }
}
