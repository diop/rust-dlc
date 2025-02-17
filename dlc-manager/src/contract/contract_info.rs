//! #ContractInfo

use super::utils::get_majority_combination;
use super::AdaptorInfo;
use super::ContractDescriptor;
use crate::error::Error;
use bitcoin::{Script, Transaction};
use dlc::{OracleInfo, Payout};
use dlc_messages::oracle_msgs::{EventDescriptor, OracleAnnouncement};
use dlc_trie::combination_iterator::CombinationIterator;
use dlc_trie::{DlcTrie, RangeInfo};
use secp256k1_zkp::{
    bitcoin_hashes::sha256, All, EcdsaAdaptorSignature, Message, PublicKey, Secp256k1, SecretKey,
    Verification,
};

pub(super) type OracleIndexAndPrefixLength = Vec<(usize, usize)>;

/// Contains information about the contract conditions and oracles used.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "camelCase")
)]
pub struct ContractInfo {
    /// The descriptor for the contract
    pub contract_descriptor: ContractDescriptor,
    /// The oracle announcements used for the contract.
    pub oracle_announcements: Vec<OracleAnnouncement>,
    /// How many oracles are required to provide a compatible outcome to be able
    /// to close the contract.
    pub threshold: usize,
}

impl ContractInfo {
    /// Get the payouts associated with the contract.
    pub fn get_payouts(&self, total_collateral: u64) -> Vec<Payout> {
        match &self.contract_descriptor {
            ContractDescriptor::Enum(e) => e.get_payouts(),
            ContractDescriptor::Numerical(n) => n.get_payouts(total_collateral),
        }
    }

    /// Utility function returning a set of OracleInfo created using the set
    /// of oracle announcements defined for the contract.
    pub fn get_oracle_infos(&self) -> Vec<OracleInfo> {
        self.oracle_announcements.iter().map(|x| x.into()).collect()
    }

    /// Uses the provided AdaptorInfo and SecretKey to generate the set of
    /// adaptor signatures for the contract.
    pub fn get_adaptor_signatures(
        &self,
        secp: &Secp256k1<All>,
        adaptor_info: &AdaptorInfo,
        fund_privkey: &SecretKey,
        funding_script_pubkey: &Script,
        fund_output_value: u64,
        cets: &[Transaction],
    ) -> Result<Vec<EcdsaAdaptorSignature>, Error> {
        match adaptor_info {
            AdaptorInfo::Enum => match &self.contract_descriptor {
                ContractDescriptor::Enum(e) => e.get_adaptor_signatures(
                    secp,
                    &self.get_oracle_infos(),
                    self.threshold,
                    cets,
                    fund_privkey,
                    funding_script_pubkey,
                    fund_output_value,
                ),
                _ => unreachable!(),
            },
            AdaptorInfo::Numerical(trie) => Ok(trie.sign(
                secp,
                fund_privkey,
                funding_script_pubkey,
                fund_output_value,
                cets,
                &self.precompute_points(secp)?,
            )?),
            AdaptorInfo::NumericalWithDifference(trie) => Ok(trie.sign(
                secp,
                fund_privkey,
                funding_script_pubkey,
                fund_output_value,
                cets,
                &self.precompute_points(secp)?,
            )?),
        }
    }

    /// Generate the AdaptorInfo for the contract while verifying the provided
    /// set of adaptor signatures.
    pub fn verify_and_get_adaptor_info(
        &self,
        secp: &Secp256k1<All>,
        total_collateral: u64,
        fund_pubkey: &PublicKey,
        funding_script_pubkey: &Script,
        fund_output_value: u64,
        cets: &[Transaction],
        adaptor_sigs: &[EcdsaAdaptorSignature],
        adaptor_sig_start: usize,
    ) -> Result<(AdaptorInfo, usize), Error> {
        let oracle_infos = self.get_oracle_infos();
        match &self.contract_descriptor {
            ContractDescriptor::Enum(e) => Ok(e.verify_and_get_adaptor_info(
                secp,
                &oracle_infos,
                self.threshold,
                fund_pubkey,
                funding_script_pubkey,
                fund_output_value,
                cets,
                adaptor_sigs,
                adaptor_sig_start,
            )?),
            ContractDescriptor::Numerical(n) => Ok(n.verify_and_get_adaptor_info(
                secp,
                total_collateral,
                fund_pubkey,
                funding_script_pubkey,
                fund_output_value,
                self.threshold,
                &self.precompute_points(secp)?,
                cets,
                adaptor_sigs,
                adaptor_sig_start,
            )?),
        }
    }

    /// Tries to find a match in the given adaptor info for the given outcomes.
    pub fn get_range_info_for_outcome(
        &self,
        adaptor_info: &AdaptorInfo,
        outcomes: &[(usize, &Vec<String>)],
        adaptor_sig_start: usize,
    ) -> Result<Option<(OracleIndexAndPrefixLength, RangeInfo)>, crate::error::Error> {
        let get_digits_outcome = |input: &[String]| -> Result<Vec<usize>, crate::error::Error> {
            input
                .iter()
                .map(|x| {
                    x.parse::<usize>().map_err(|_| {
                        crate::error::Error::InvalidParameters(
                            "Invalid outcome, {} is not a valid number.".to_string(),
                        )
                    })
                })
                .collect::<Result<Vec<usize>, crate::error::Error>>()
        };

        match adaptor_info {
            AdaptorInfo::Enum => match &self.contract_descriptor {
                ContractDescriptor::Enum(e) => e.get_range_info_for_outcome(
                    self.oracle_announcements.len(),
                    self.threshold,
                    outcomes,
                    adaptor_sig_start,
                ),
                _ => unreachable!(),
            },
            AdaptorInfo::Numerical(n) => {
                let (s_outcomes, actual_combination) = get_majority_combination(outcomes)?;
                let digits_outcome = get_digits_outcome(&s_outcomes)?;

                let res = n
                    .digit_trie
                    .look_up(&digits_outcome)
                    .ok_or(crate::error::Error::InvalidState)?;

                let sufficient_combination: Vec<_> = actual_combination
                    .into_iter()
                    .take(self.threshold)
                    .collect();
                let position =
                    CombinationIterator::new(self.oracle_announcements.len(), self.threshold)
                        .get_index_for_combination(&sufficient_combination)
                        .ok_or(crate::error::Error::InvalidState)?;
                Ok(Some((
                    sufficient_combination
                        .iter()
                        .map(|x| (*x, res[0].path.len()))
                        .collect(),
                    res[0].value[position].clone(),
                )))
            }
            AdaptorInfo::NumericalWithDifference(n) => {
                let res = n
                    .multi_trie
                    .look_up(
                        &outcomes
                            .iter()
                            .map(|(x, path)| Ok((*x, get_digits_outcome(path)?)))
                            .collect::<Result<Vec<(usize, Vec<usize>)>, crate::error::Error>>()?,
                    )
                    .ok_or(crate::error::Error::InvalidState)?;
                Ok(Some((
                    res.path.iter().map(|(x, y)| (*x, y.len())).collect(),
                    res.value.clone(),
                )))
            }
        }
    }

    /// Verifies the given adaptor signatures are valid with respect to the given
    /// adaptor info.
    pub fn verify_adaptor_info(
        &self,
        secp: &Secp256k1<All>,
        fund_pubkey: &PublicKey,
        funding_script_pubkey: &Script,
        fund_output_value: u64,
        cets: &[Transaction],
        adaptor_sigs: &[EcdsaAdaptorSignature],
        adaptor_sig_start: usize,
        adaptor_info: &AdaptorInfo,
    ) -> Result<usize, Error> {
        let oracle_infos = self.get_oracle_infos();
        match &self.contract_descriptor {
            ContractDescriptor::Enum(e) => Ok(e.verify_adaptor_info(
                secp,
                &oracle_infos,
                self.threshold,
                fund_pubkey,
                funding_script_pubkey,
                fund_output_value,
                cets,
                adaptor_sigs,
                adaptor_sig_start,
            )?),
            ContractDescriptor::Numerical(_) => match adaptor_info {
                AdaptorInfo::Enum => unreachable!(),
                AdaptorInfo::Numerical(trie) => Ok(trie.verify(
                    secp,
                    fund_pubkey,
                    funding_script_pubkey,
                    fund_output_value,
                    adaptor_sigs,
                    cets,
                    &self.precompute_points(secp)?,
                )?),
                AdaptorInfo::NumericalWithDifference(trie) => Ok(trie.verify(
                    secp,
                    fund_pubkey,
                    funding_script_pubkey,
                    fund_output_value,
                    adaptor_sigs,
                    cets,
                    &self.precompute_points(secp)?,
                )?),
            },
        }
    }

    /// Generate the adaptor info and adaptor signatures for the contract.
    pub fn get_adaptor_info(
        &self,
        secp: &Secp256k1<All>,
        total_collateral: u64,
        fund_priv_key: &SecretKey,
        funding_script_pubkey: &Script,
        fund_output_value: u64,
        cets: &[Transaction],
        adaptor_index_start: usize,
    ) -> Result<(AdaptorInfo, Vec<EcdsaAdaptorSignature>), Error> {
        match &self.contract_descriptor {
            ContractDescriptor::Enum(e) => {
                let oracle_infos = self.get_oracle_infos();
                Ok(e.get_adaptor_info(
                    secp,
                    &oracle_infos,
                    self.threshold,
                    fund_priv_key,
                    funding_script_pubkey,
                    fund_output_value,
                    cets,
                )?)
            }
            ContractDescriptor::Numerical(n) => Ok(n.get_adaptor_info(
                secp,
                total_collateral,
                fund_priv_key,
                funding_script_pubkey,
                fund_output_value,
                self.threshold,
                &self.precompute_points(secp)?,
                cets,
                adaptor_index_start,
            )?),
        }
    }

    fn precompute_points<C: Verification>(
        &self,
        secp: &Secp256k1<C>,
    ) -> Result<Vec<Vec<Vec<PublicKey>>>, Error> {
        self.oracle_announcements
            .iter()
            .map(|x| {
                let pubkey = &x.oracle_public_key;
                let nonces = &x.oracle_event.oracle_nonces;
                match &x.oracle_event.event_descriptor {
                    EventDescriptor::DigitDecompositionEvent(d) => {
                        let base = d.base as usize;
                        let nb_digits = d.nb_digits as usize;
                        if nb_digits != nonces.len() {
                            return Err(Error::InvalidParameters(
                                "Number of digits and nonces must be equal".to_string(),
                            ));
                        }
                        let mut d_points = Vec::with_capacity(nb_digits);
                        for nonce in nonces {
                            let mut points = Vec::with_capacity(base);
                            for j in 0..base {
                                let msg = Message::from_hashed_data::<sha256::Hash>(
                                    j.to_string().as_bytes(),
                                );
                                let sig_point = dlc::secp_utils::schnorrsig_compute_sig_point(
                                    secp, pubkey, nonce, &msg,
                                )?;
                                points.push(sig_point);
                            }
                            d_points.push(points);
                        }
                        Ok(d_points)
                    }
                    _ => Err(Error::InvalidParameters(
                        "Expected digit decomposition event.".to_string(),
                    )),
                }
            })
            .collect::<Result<Vec<Vec<Vec<PublicKey>>>, Error>>()
    }
}
