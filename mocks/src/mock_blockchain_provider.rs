use bitcoin::Transaction;
use dlc_manager::{error::Error, Blockchain};

pub struct MockBlockchainProvider {}

impl Blockchain for MockBlockchainProvider {
    fn send_transaction(&self, _: &Transaction) -> Result<(), Error> {
        Ok(())
    }
    fn get_network(&self) -> Result<bitcoin::network::constants::Network, Error> {
        Ok(bitcoin::network::constants::Network::Regtest)
    }
}
