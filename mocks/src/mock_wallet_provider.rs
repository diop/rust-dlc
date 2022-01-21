use bitcoin::hashes::hex::FromHex;
use bitcoin::{network::constants::Network, Address, OutPoint, Script, Transaction, TxOut, Txid};
use dlc_manager::error::Error;
use dlc_manager::{Utxo, Wallet};
use secp256k1_zkp::global::SECP256K1;
use secp256k1_zkp::{key::ONE_KEY, PublicKey, SecretKey};
use std::collections::HashMap;
use std::sync::Mutex;

pub struct MockWallet {
    key_map: Mutex<HashMap<PublicKey, SecretKey>>,
    address_map: Mutex<HashMap<Address, SecretKey>>,
}

impl MockWallet {
    pub fn new() -> Self {
        MockWallet {
            key_map: Mutex::new(HashMap::new()),
            address_map: Mutex::new(HashMap::new()),
        }
    }
}

impl Wallet for MockWallet {
    fn get_new_address(&self) -> Result<Address, Error> {
        let seckey = ONE_KEY.clone();
        let privkey = bitcoin::PrivateKey::new(seckey, Network::Regtest);
        let pubkey = bitcoin::PublicKey::from_private_key(&SECP256K1, &privkey);
        let address = Address::p2wpkh(&pubkey, Network::Regtest).expect("to yield a valid address");
        self.address_map
            .lock()
            .unwrap()
            .insert(address.clone(), seckey);
        Ok(address)
    }

    fn get_new_secret_key(&self) -> Result<SecretKey, Error> {
        let seckey = ONE_KEY.clone();
        let pubkey = PublicKey::from_secret_key(&SECP256K1, &seckey);
        self.key_map.lock().unwrap().insert(pubkey, seckey);
        Ok(seckey)
    }

    fn get_secret_key_for_pubkey(&self, pubkey: &PublicKey) -> Result<SecretKey, Error> {
        Ok(*self
            .key_map
            .lock()
            .unwrap()
            .get(pubkey)
            .expect("to have the queried key"))
    }

    /// Get a set of UTXOs to fund the given amount.
    fn get_utxos_for_amount(
        &self,
        amount: u64,
        _: Option<u64>,
        _: bool,
    ) -> Result<Vec<Utxo>, Error> {
        Ok(vec![Utxo {
            address: self.get_new_address().expect("to yield an address"),
            outpoint: OutPoint {
                txid: Txid::from_hex(
                    "5df6e0e2761359d30a8275058e299fcc0381534545f55cf43e41983f5d4c9456",
                )
                .unwrap(),
                vout: 0,
            },
            redeem_script: Script::new(),
            tx_out: TxOut {
                value: amount,
                script_pubkey: Script::new(),
            },
        }])
    }

    fn sign_tx_input(
        &self,
        _: &mut Transaction,
        _: usize,
        _: &TxOut,
        _: Option<Script>,
    ) -> Result<(), Error> {
        unimplemented!();
    }

    fn import_address(&self, _: &Address) -> Result<(), Error> {
        Ok(())
    }

    fn get_transaction(&self, _: &Txid) -> Result<Transaction, Error> {
        unimplemented!();
    }

    fn get_transaction_confirmations(&self, _: &Txid) -> Result<u32, Error> {
        Ok(2)
    }
}
