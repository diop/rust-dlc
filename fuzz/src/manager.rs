use dlc_manager::manager::Manager;
use dlc_messages::{Message, OfferDlc};
use lightning::util::ser::Readable;
use secp256k1_zkp::schnorrsig::PublicKey;
use std::str::FromStr;

pub fn manager_run(data: &[u8]) {
    let mut buf = ::std::io::Cursor::new(data);
    if let Ok(msg) = <OfferDlc as Readable>::read(&mut buf) {
        let store = mocks::memory_storage_provider::MemoryStorage::new();
        let mock_time = mocks::mock_time::MockTime {};
        mocks::mock_time::set_time(1642740576);
        let mock_blockchain = mocks::mock_blockchain_provider::MockBlockchainProvider {};
        let mock_wallet = mocks::mock_wallet_provider::MockWallet::new();
        let mock_oracles =
            std::collections::HashMap::<PublicKey, &mocks::mock_oracle_provider::MockOracle>::new();

        let mut manager = Manager::new(
            &mock_wallet,
            &mock_blockchain,
            Box::new(store),
            mock_oracles,
            &mock_time,
        );
        let temporary_contract_id = msg.get_hash().unwrap();
        let secret_key = secp256k1_zkp::SecretKey::from_str(
            "0000000000000000000000000000000000000000000000000000000000000001",
        )
        .unwrap();
        let pk = secp256k1_zkp::PublicKey::from_secret_key(&secp256k1_zkp::SECP256K1, &secret_key);
        if let Ok(_) = manager.on_dlc_message(&Message::Offer(msg), pk) {
            if let Ok(_) = manager.accept_contract_offer(&temporary_contract_id) {}
        }
    }
}
