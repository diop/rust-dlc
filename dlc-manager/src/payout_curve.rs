//! #PayoutFunction

use crate::error::Error;
use dlc::{Payout, RangePayout};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Contains information to compute the set of payouts based on the outcomes.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "camelCase")
)]
pub struct PayoutFunction {
    /// The pieces making up the function.
    pub(crate) payout_function_pieces: Vec<PayoutFunctionPiece>,
}

impl PayoutFunction {
    /// Create a new payout function
    pub fn new(function_pieces: Vec<PayoutFunctionPiece>) -> Result<PayoutFunction, Error> {
        let is_continuous = function_pieces
            .iter()
            .zip(function_pieces.iter().skip(1))
            .all(|(cur, next)| cur.get_last_point() == next.get_first_point());
        if is_continuous {
            Ok(PayoutFunction {
                payout_function_pieces: function_pieces,
            })
        } else {
            Err(Error::InvalidParameters(
                "Function pieces are not continuous.".to_string(),
            ))
        }
    }

    /// Generate the range payouts from the function.
    pub fn to_range_payouts(
        &self,
        total_collateral: u64,
        rounding_intervals: &RoundingIntervals,
    ) -> Vec<RangePayout> {
        let mut range_payouts = Vec::new();
        for piece in &self.payout_function_pieces {
            piece.to_range_payouts(total_collateral, rounding_intervals, &mut range_payouts);
        }
        range_payouts
    }
}

/// A piece of a payout function.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "camelCase")
)]
pub enum PayoutFunctionPiece {
    /// A function piece represented by a polynomial.
    PolynomialPayoutCurvePiece(PolynomialPayoutCurvePiece),
    /// A function piece represented by an hyperbola.
    HyperbolaPayoutCurvePiece(HyperbolaPayoutCurvePiece),
}

impl PayoutFunctionPiece {
    /// Generate the range payouts for the function piece.
    pub fn to_range_payouts(
        &self,
        total_collateral: u64,
        rounding_intervals: &RoundingIntervals,
        range_payouts: &mut Vec<RangePayout>,
    ) {
        match self {
            PayoutFunctionPiece::PolynomialPayoutCurvePiece(p) => {
                p.to_range_payouts(rounding_intervals, total_collateral, range_payouts)
            }
            PayoutFunctionPiece::HyperbolaPayoutCurvePiece(h) => {
                h.to_range_payouts(rounding_intervals, total_collateral, range_payouts)
            }
        }
    }

    fn get_first_point(&self) -> &PayoutPoint {
        match self {
            PayoutFunctionPiece::PolynomialPayoutCurvePiece(p) => &p.payout_points[0],
            PayoutFunctionPiece::HyperbolaPayoutCurvePiece(h) => &h.left_end_point,
        }
    }

    fn get_last_point(&self) -> &PayoutPoint {
        match self {
            PayoutFunctionPiece::PolynomialPayoutCurvePiece(p) => p.payout_points.last().unwrap(),
            PayoutFunctionPiece::HyperbolaPayoutCurvePiece(h) => &h.right_end_point,
        }
    }
}

trait Evaluable {
    fn evaluate(&self, outcome: u64) -> f64;

    fn get_rounded_payout(&self, outcome: u64, rounding_intervals: &RoundingIntervals) -> u64 {
        let payout_double = self.evaluate(outcome);
        rounding_intervals.round(outcome, payout_double)
    }

    fn get_first_outcome(&self) -> u64;

    fn get_last_outcome(&self) -> u64;

    fn to_range_payouts(
        &self,
        rounding_intervals: &RoundingIntervals,
        total_collateral: u64,
        range_payouts: &mut Vec<RangePayout>,
    ) {
        let first_outcome = self.get_first_outcome();
        let mut cur_range = range_payouts.pop().unwrap_or_else(|| {
            let first_payout = self.get_rounded_payout(first_outcome, rounding_intervals);
            RangePayout {
                start: first_outcome as usize,
                count: 1,
                payout: Payout {
                    offer: first_payout,
                    accept: total_collateral - first_payout,
                },
            }
        });

        for outcome in (first_outcome + 1)..(self.get_last_outcome() + 1) {
            let payout = self.get_rounded_payout(outcome, rounding_intervals);
            if cur_range.payout.offer == payout {
                cur_range.count += 1;
            } else {
                range_payouts.push(cur_range);
                cur_range = RangePayout {
                    start: outcome as usize,
                    count: 1,
                    payout: Payout {
                        offer: payout,
                        accept: total_collateral - payout,
                    },
                };
            }
        }

        range_payouts.push(cur_range);
    }
}

/// A function piece represented by a polynomial.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "camelCase")
)]
pub struct PolynomialPayoutCurvePiece {
    /// The set of points to be used to interpolate the polynomial.
    pub(crate) payout_points: Vec<PayoutPoint>,
}

impl PolynomialPayoutCurvePiece {
    /// Create a new PolynomialPayoutCurvePiece
    pub fn new(payout_points: Vec<PayoutPoint>) -> Result<Self, Error> {
        let is_ascending = payout_points.len() > 1
            && payout_points
                .iter()
                .zip(payout_points.iter().skip(1))
                .all(|(cur, next)| cur.event_outcome < next.event_outcome);
        if is_ascending {
            Ok(PolynomialPayoutCurvePiece { payout_points })
        } else {
            Err(Error::InvalidParameters(
                "Payout points must have ascending event outcome value.".to_string(),
            ))
        }
    }
}

impl Evaluable for PolynomialPayoutCurvePiece {
    fn evaluate(&self, outcome: u64) -> f64 {
        let nb_points = self.payout_points.len() as usize;
        let mut result = 0.0;
        let outcome = outcome as f64;

        for i in 0..nb_points {
            let mut l = self.payout_points[i].get_outcome_payout() as f64;
            for j in 0..nb_points {
                if i != j {
                    debug_assert!(
                        self.payout_points[i].event_outcome != self.payout_points[j].event_outcome
                    );
                    let i_outcome = self.payout_points[i].event_outcome as f64;
                    let j_outcome = self.payout_points[j].event_outcome as f64;
                    let denominator = i_outcome - j_outcome;
                    let numerator = outcome - j_outcome;
                    l *= numerator / denominator;
                }
            }
            result += l;
        }

        result
    }

    fn get_first_outcome(&self) -> u64 {
        self.payout_points[0].event_outcome
    }

    fn get_last_outcome(&self) -> u64 {
        self.payout_points.last().unwrap().event_outcome
    }
}

/// A payout point representing a payout for a given outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "camelCase")
)]
pub struct PayoutPoint {
    /// The event outcome.
    pub event_outcome: u64,
    /// The payout for the outcome.
    pub outcome_payout: u64,
    /// Extra precision to use when computing the payout.
    pub extra_precision: u16,
}

impl PayoutPoint {
    fn get_outcome_payout(&self) -> f64 {
        (self.outcome_payout as f64) + ((self.extra_precision as f64) / ((1 << 16) as f64))
    }
}

/// A function piece represented by a hyperbola.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "camelCase")
)]
pub struct HyperbolaPayoutCurvePiece {
    /// The left end point of the piece.
    pub(crate) left_end_point: PayoutPoint,
    /// The right end point of the piece.
    pub(crate) right_end_point: PayoutPoint,
    /// Which piece to use in case of ambiguity.
    pub(crate) use_positive_piece: bool,
    /// X coordinate of the translation point.
    pub(crate) translate_outcome: f64,
    /// Y coordinate of the translation point.
    pub(crate) translate_payout: f64,
    /// a value of the transformation matrix.
    pub(crate) a: f64,
    /// b value of the transformation matrix.
    pub(crate) b: f64,
    /// c value of the transformation matrix.
    pub(crate) c: f64,
    /// d value of the transformation matrix.
    pub(crate) d: f64,
}

impl HyperbolaPayoutCurvePiece {
    /// Create a new HyperbolaPayoutCurvePiece
    pub fn new(
        left_end_point: PayoutPoint,
        right_end_point: PayoutPoint,
        use_positive_piece: bool,
        translate_outcome: f64,
        translate_payout: f64,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
    ) -> Result<Self, Error> {
        if a * b == d * c {
            Err(Error::InvalidParameters(
                "a * c cannot equal d * c".to_string(),
            ))
        } else if left_end_point.event_outcome >= right_end_point.event_outcome {
            Err(Error::InvalidParameters(
                "Left end point outcome must be strictly less than right end point outcome"
                    .to_string(),
            ))
        } else {
            Ok(HyperbolaPayoutCurvePiece {
                left_end_point,
                right_end_point,
                use_positive_piece,
                translate_outcome,
                translate_payout,
                a,
                b,
                c,
                d,
            })
        }
    }
}

impl Evaluable for HyperbolaPayoutCurvePiece {
    fn evaluate(&self, outcome: u64) -> f64 {
        let outcome = outcome as f64;
        let translated_outcome = outcome as f64 - self.translate_outcome;
        let sqrt_term_abs_val = (translated_outcome.powi(2) - 4.0 * self.a * self.b).sqrt();
        let sqrt_term = if self.use_positive_piece {
            sqrt_term_abs_val
        } else {
            -sqrt_term_abs_val
        };

        let first_term = self.c * (translated_outcome + sqrt_term) / (2.0 * self.a);
        let second_term = 2.0 * self.a * self.d / (translated_outcome + sqrt_term);
        first_term + second_term + self.translate_payout
    }

    fn get_first_outcome(&self) -> u64 {
        self.left_end_point.event_outcome
    }
    fn get_last_outcome(&self) -> u64 {
        self.right_end_point.event_outcome
    }
}

/// Provides information on if and how to round the payouts of a payout function
/// to reduce the number of adaptor signatures required. A `rounding_mod` value
/// of 1 indicates that no rounding is performed.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "camelCase")
)]
pub struct RoundingInterval {
    /// The start of the rounding interval.
    pub begin_interval: u64,
    /// The rounding modulus value.
    pub rounding_mod: u64,
}

/// A set of rounding intervals.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "camelCase")
)]
pub struct RoundingIntervals {
    /// Contains the rounding intervals.
    pub intervals: Vec<RoundingInterval>,
}

impl RoundingIntervals {
    /// Round the given payout based on the rounding modulus matching the given
    /// outcome.
    pub fn round(&self, outcome: u64, payout: f64) -> u64 {
        let rounding_mod = match self
            .intervals
            .binary_search_by(|x| x.begin_interval.cmp(&outcome))
        {
            Ok(index) => self.intervals[index].rounding_mod,
            Err(index) if index != 0 => self.intervals[index - 1].rounding_mod,
            _ => unreachable!(),
        } as f64;

        let m = if payout >= 0.0 {
            payout % rounding_mod
        } else {
            payout % rounding_mod + rounding_mod
        };

        if m >= rounding_mod / 2.0 {
            (payout + rounding_mod - m).round() as u64
        } else {
            (payout - m).round() as u64
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use secp256k1_zkp::rand::{thread_rng, RngCore};

    #[test]
    fn lagrange_interpolate_test() {
        let polynomial = PolynomialPayoutCurvePiece {
            payout_points: vec![
                PayoutPoint {
                    event_outcome: 0,
                    outcome_payout: 1,
                    extra_precision: 0,
                },
                PayoutPoint {
                    event_outcome: 2,
                    outcome_payout: 5,
                    extra_precision: 0,
                },
                PayoutPoint {
                    event_outcome: 4,
                    outcome_payout: 17,
                    extra_precision: 0,
                },
            ],
        };

        assert_eq!(101_f64, polynomial.evaluate(10));
        assert_eq!(10001_f64, polynomial.evaluate(100));
    }

    #[test]
    fn polynomial_to_range_outcome_test() {
        struct TestCase {
            payout_points: Vec<PayoutPoint>,
            expected_len: usize,
            expected_first_start: usize,
            expected_first_payout: u64,
            expected_last_start: usize,
            expected_last_payout: u64,
            total_collateral: u64,
        }
        let test_cases: Vec<TestCase> = vec![
            TestCase {
                payout_points: vec![
                    PayoutPoint {
                        event_outcome: 0,
                        outcome_payout: 0,
                        extra_precision: 0,
                    },
                    PayoutPoint {
                        event_outcome: 20,
                        outcome_payout: 20,
                        extra_precision: 0,
                    },
                ],
                expected_len: 21,
                expected_first_start: 0,
                expected_first_payout: 0,
                expected_last_start: 20,
                expected_last_payout: 20,
                total_collateral: 20,
            },
            TestCase {
                payout_points: vec![
                    PayoutPoint {
                        event_outcome: 10,
                        outcome_payout: 10,
                        extra_precision: 0,
                    },
                    PayoutPoint {
                        event_outcome: 20,
                        outcome_payout: 10,
                        extra_precision: 0,
                    },
                ],
                expected_len: 1,
                expected_first_start: 10,
                expected_first_payout: 10,
                expected_last_start: 10,
                expected_last_payout: 10,
                total_collateral: 10,
            },
            TestCase {
                payout_points: vec![
                    PayoutPoint {
                        event_outcome: 50000,
                        outcome_payout: 0,
                        extra_precision: 0,
                    },
                    PayoutPoint {
                        event_outcome: 1048575,
                        outcome_payout: 0,
                        extra_precision: 0,
                    },
                ],
                expected_len: 1,
                expected_first_start: 50000,
                expected_first_payout: 0,
                expected_last_start: 50000,
                expected_last_payout: 0,
                total_collateral: 200000000,
            },
        ];

        for test_case in test_cases {
            let polynomial = PolynomialPayoutCurvePiece {
                payout_points: test_case.payout_points,
            };

            let rounding_intervals = RoundingIntervals {
                intervals: vec![RoundingInterval {
                    begin_interval: 0,
                    rounding_mod: 1,
                }],
            };

            let mut range_payouts = Vec::new();
            polynomial.to_range_payouts(
                &rounding_intervals,
                test_case.total_collateral,
                &mut range_payouts,
            );
            let first = range_payouts.first().unwrap();
            let last = range_payouts.last().unwrap();

            assert_eq!(test_case.expected_len, range_payouts.len());
            assert_eq!(test_case.expected_first_start, first.start);
            assert_eq!(test_case.expected_first_payout, first.payout.offer);
            assert_eq!(test_case.expected_last_start, last.start);
            assert_eq!(test_case.expected_last_payout, last.payout.offer);
        }
    }

    #[test]
    fn hyperbola_test() {
        let d = (thread_rng().next_u64() as f64) + (thread_rng().next_u64() as f64 / 100.0);
        let translate_payout =
            (thread_rng().next_u64() as f64) + (thread_rng().next_u64() as f64 / 100.0);
        let outcomes: Vec<_> = (0..100).map(|_| thread_rng().next_u64()).collect();
        let expected_payout = |outcome: u64| -> f64 { d / outcome as f64 + translate_payout };

        let hyperbola = HyperbolaPayoutCurvePiece {
            left_end_point: PayoutPoint {
                event_outcome: 0,
                outcome_payout: 0,
                extra_precision: 0,
            },
            right_end_point: PayoutPoint {
                event_outcome: u64::MAX,
                outcome_payout: 0,
                extra_precision: 0,
            },
            use_positive_piece: true,
            translate_outcome: 0.0,
            translate_payout,
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d,
        };

        for outcome in outcomes {
            assert_eq!(expected_payout(outcome), hyperbola.evaluate(outcome));
        }
    }

    #[test]
    fn payout_function_to_range_outcome_test() {
        let payout_function = PayoutFunction::new(vec![
            PayoutFunctionPiece::PolynomialPayoutCurvePiece(
                PolynomialPayoutCurvePiece::new(vec![
                    PayoutPoint {
                        event_outcome: 0,
                        outcome_payout: 0,
                        extra_precision: 0,
                    },
                    PayoutPoint {
                        event_outcome: 9,
                        outcome_payout: 0,
                        extra_precision: 0,
                    },
                ])
                .unwrap(),
            ),
            PayoutFunctionPiece::PolynomialPayoutCurvePiece(
                PolynomialPayoutCurvePiece::new(vec![
                    PayoutPoint {
                        event_outcome: 9,
                        outcome_payout: 0,
                        extra_precision: 0,
                    },
                    PayoutPoint {
                        event_outcome: 10,
                        outcome_payout: 10,
                        extra_precision: 0,
                    },
                ])
                .unwrap(),
            ),
            PayoutFunctionPiece::PolynomialPayoutCurvePiece(
                PolynomialPayoutCurvePiece::new(vec![
                    PayoutPoint {
                        event_outcome: 10,
                        outcome_payout: 10,
                        extra_precision: 0,
                    },
                    PayoutPoint {
                        event_outcome: 20,
                        outcome_payout: 10,
                        extra_precision: 0,
                    },
                ])
                .unwrap(),
            ),
        ])
        .unwrap();
        let expected_ranges = vec![
            RangePayout {
                start: 0,
                count: 10,
                payout: Payout {
                    offer: 0,
                    accept: 10,
                },
            },
            RangePayout {
                start: 10,
                count: 11,
                payout: Payout {
                    offer: 10,
                    accept: 0,
                },
            },
        ];
        assert_eq!(
            expected_ranges,
            payout_function.to_range_payouts(
                10,
                &RoundingIntervals {
                    intervals: vec![RoundingInterval {
                        begin_interval: 0,
                        rounding_mod: 1
                    }]
                }
            )
        );
    }

    #[test]
    fn polynomial_payout_curve_validity_test() {
        let invalid = vec![
            // Polynomial curve piece requires more than one
            vec![PayoutPoint {
                event_outcome: 0,
                outcome_payout: 0,
                extra_precision: 0,
            }],
            // Payout point outcomes should be increasing
            vec![
                PayoutPoint {
                    event_outcome: 10,
                    outcome_payout: 0,
                    extra_precision: 0,
                },
                PayoutPoint {
                    event_outcome: 9,
                    outcome_payout: 0,
                    extra_precision: 0,
                },
            ],
        ];

        for points in invalid {
            PolynomialPayoutCurvePiece::new(points).expect_err("Invalid pieces should error");
        }
    }

    #[test]
    fn hyperbola_validity_test() {
        HyperbolaPayoutCurvePiece::new(
            PayoutPoint {
                event_outcome: 0,
                outcome_payout: 0,
                extra_precision: 0,
            },
            PayoutPoint {
                event_outcome: 0,
                outcome_payout: 0,
                extra_precision: 0,
            },
            true,
            0.0,
            0.0,
            1.0,
            2.0,
            3.0,
            4.0,
        )
        .expect_err("Payout point are not increasing should error.");
        HyperbolaPayoutCurvePiece::new(
            PayoutPoint {
                event_outcome: 0,
                outcome_payout: 0,
                extra_precision: 0,
            },
            PayoutPoint {
                event_outcome: 1,
                outcome_payout: 0,
                extra_precision: 0,
            },
            true,
            0.0,
            0.0,
            1.0,
            2.0,
            1.0,
            2.0,
        )
        .expect_err("a * b == d * c should error.");
    }

    #[test]
    fn payout_function_validity_test() {
        let invalid = vec![
            // Pieces should form a continuous function
            vec![
                PayoutFunctionPiece::PolynomialPayoutCurvePiece(PolynomialPayoutCurvePiece {
                    payout_points: vec![
                        PayoutPoint {
                            event_outcome: 0,
                            outcome_payout: 0,
                            extra_precision: 0,
                        },
                        PayoutPoint {
                            event_outcome: 9,
                            outcome_payout: 0,
                            extra_precision: 0,
                        },
                    ],
                }),
                PayoutFunctionPiece::PolynomialPayoutCurvePiece(PolynomialPayoutCurvePiece {
                    payout_points: vec![
                        PayoutPoint {
                            event_outcome: 11,
                            outcome_payout: 0,
                            extra_precision: 0,
                        },
                        PayoutPoint {
                            event_outcome: 19,
                            outcome_payout: 0,
                            extra_precision: 0,
                        },
                    ],
                }),
            ],
            vec![
                PayoutFunctionPiece::PolynomialPayoutCurvePiece(PolynomialPayoutCurvePiece {
                    payout_points: vec![
                        PayoutPoint {
                            event_outcome: 0,
                            outcome_payout: 0,
                            extra_precision: 0,
                        },
                        PayoutPoint {
                            event_outcome: 9,
                            outcome_payout: 0,
                            extra_precision: 0,
                        },
                    ],
                }),
                PayoutFunctionPiece::PolynomialPayoutCurvePiece(PolynomialPayoutCurvePiece {
                    payout_points: vec![
                        PayoutPoint {
                            event_outcome: 10,
                            outcome_payout: 1,
                            extra_precision: 0,
                        },
                        PayoutPoint {
                            event_outcome: 19,
                            outcome_payout: 1,
                            extra_precision: 0,
                        },
                    ],
                }),
            ],
        ];

        for pieces in invalid {
            PayoutFunction::new(pieces).expect_err("Invalid pieces should error");
        }
    }
}
