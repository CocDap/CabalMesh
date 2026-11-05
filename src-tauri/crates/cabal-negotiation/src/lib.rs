//! The rules of a price negotiation between two agents.
//!
//! This owns turns, rounds, timeouts and the user's price limit, and does no I/O,
//! so a model can suggest anything and only what passes here reaches the mesh.
//!
//! # How a negotiation runs
//!
//! The seller opens. A round is the seller's price and the buyer's reply. Each side
//! accepts by naming the other's last price, and a deal needs both to have sent
//! `Accept` at the same price: the side that hears an `Accept` for its own last price
//! answers with its own, so both know the same thing before anyone signs.

use cabal_core::Action;
use serde::{Deserialize, Serialize};

pub const MAX_ROUNDS: u8 = 3;
pub const ROUND_TIMEOUT_MS: u64 = 60_000;

/// How far from the market the fallback will go for a user who set no limit.
const NO_LIMIT_MARGIN_PERCENT: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

impl TryFrom<Action> for Side {
    type Error = ();

    fn try_from(action: Action) -> Result<Self, ()> {
        match action {
            Action::Buy => Ok(Self::Buy),
            Action::Sell => Ok(Self::Sell),
            Action::Swap | Action::Stake => Err(()),
        }
    }
}

/// What this user will accept: a buyer's maximum or a seller's minimum. Stays on the
/// device. `None` means any price.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bound {
    pub side: Side,
    pub limit_cents: Option<u64>,
}

impl Bound {
    /// Whether this user would trade at `price_cents`.
    #[must_use]
    pub fn allows(self, price_cents: u64) -> bool {
        price_cents > 0
            && self.limit_cents.is_none_or(|limit| match self.side {
                Side::Buy => price_cents <= limit,
                Side::Sell => price_cents >= limit,
            })
    }
}

/// What agents send each other, in the clear. Has no field a limit could travel in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Msg {
    Offer {
        neg_id: String,
        round: u8,
        price_cents: u64,
    },
    Counter {
        neg_id: String,
        round: u8,
        price_cents: u64,
    },
    Accept {
        neg_id: String,
        round: u8,
        price_cents: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    RoundsExhausted,
    TimedOut,
    Unmatched,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing ended. Also what a resent, stale or forged message gets.
    Continue,
    /// `reply` is this side's `Accept`, if it still has to send one.
    Agreed {
        price_cents: u64,
        reply: Option<Msg>,
    },
    Ended {
        reason: EndReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    OutsideBound,
    NotOurTurn,
    Ended,
}

#[derive(Debug, Clone)]
pub struct Negotiation {
    neg_id: String,
    own: Bound,
    round: u8,
    our_turn: bool,
    last_ours: Option<u64>,
    last_theirs: Option<u64>,
    /// The price this side sent `Accept` for.
    accepted: Option<u64>,
    deadline_ms: u64,
    ended: Option<EndReason>,
    agreed: bool,
}

impl Negotiation {
    #[must_use]
    pub fn start(neg_id: String, own: Bound, now_ms: u64) -> Self {
        Self {
            neg_id,
            own,
            round: 1,
            our_turn: own.side == Side::Sell,
            last_ours: None,
            last_theirs: None,
            accepted: None,
            deadline_ms: now_ms.saturating_add(ROUND_TIMEOUT_MS),
            ended: None,
            agreed: false,
        }
    }

    /// Turns a price into the message to send: an `Accept` when it meets or beats the
    /// other side's last price, otherwise this side's next offer.
    ///
    /// # Errors
    ///
    /// [`Reject`] says why nothing may be sent. Past the last round a side may still
    /// accept, but any other price ends the negotiation.
    pub fn propose(&mut self, price_cents: u64, now_ms: u64) -> Result<Msg, Reject> {
        if self.is_over() {
            return Err(Reject::Ended);
        }
        if !self.our_turn {
            return Err(Reject::NotOurTurn);
        }
        if !self.own.allows(price_cents) {
            return Err(Reject::OutsideBound);
        }

        let neg_id = self.neg_id.clone();
        if let Some(theirs) = self
            .last_theirs
            .filter(|theirs| self.meets(price_cents, *theirs))
        {
            self.accepted = Some(theirs);
            self.spoke(now_ms);
            return Ok(Msg::Accept {
                neg_id,
                round: self.round,
                price_cents: theirs,
            });
        }

        let msg = if self.own.side == Side::Buy {
            Msg::Counter {
                neg_id,
                round: self.round,
                price_cents,
            }
        } else if self.last_ours.is_none() {
            Msg::Offer {
                neg_id,
                round: self.round,
                price_cents,
            }
        } else if self.round < MAX_ROUNDS {
            self.round += 1;
            Msg::Counter {
                neg_id,
                round: self.round,
                price_cents,
            }
        } else {
            self.ended = Some(EndReason::RoundsExhausted);
            return Err(Reject::Ended);
        };
        self.last_ours = Some(price_cents);
        self.spoke(now_ms);
        Ok(msg)
    }

    /// Takes in what the other side sent. Anything that does not fit the current
    /// state changes nothing, because this topic can resend and anyone can forge.
    pub fn receive(&mut self, msg: &Msg, now_ms: u64) -> Outcome {
        if self.is_over() {
            return Outcome::Continue;
        }
        if now_ms > self.deadline_ms {
            return self.tick(now_ms);
        }
        match msg {
            Msg::Accept {
                neg_id,
                round,
                price_cents,
            } if *neg_id == self.neg_id && *round == self.round => self.on_accept(*price_cents),
            Msg::Offer {
                neg_id,
                round,
                price_cents,
            } if *neg_id == self.neg_id => {
                if self.own.side == Side::Buy && *round == 1 && self.last_theirs.is_none() {
                    self.heard(*price_cents, 1, now_ms);
                }
                Outcome::Continue
            }
            Msg::Counter {
                neg_id,
                round,
                price_cents,
            } if *neg_id == self.neg_id
                && self.last_ours.is_some()
                && !self.our_turn
                && self.accepted.is_none() =>
            {
                let expected = match self.own.side {
                    Side::Sell => self.round,
                    Side::Buy => self.round + 1,
                };
                if *round == expected && *round <= MAX_ROUNDS {
                    self.heard(*price_cents, *round, now_ms);
                }
                Outcome::Continue
            }
            _ => Outcome::Continue,
        }
    }

    /// Ends the negotiation if the current round has run out. Reports that once.
    pub fn tick(&mut self, now_ms: u64) -> Outcome {
        if self.is_over() || now_ms <= self.deadline_ms {
            return Outcome::Continue;
        }
        self.ended = Some(EndReason::TimedOut);
        Outcome::Ended {
            reason: EndReason::TimedOut,
        }
    }

    #[must_use]
    pub fn ended(&self) -> Option<EndReason> {
        self.ended
    }

    #[must_use]
    pub const fn is_over(&self) -> bool {
        self.agreed || self.ended.is_some()
    }

    #[must_use]
    pub const fn is_our_turn(&self) -> bool {
        self.our_turn && !self.is_over()
    }

    #[must_use]
    pub const fn round(&self) -> u8 {
        self.round
    }

    #[must_use]
    pub const fn last_theirs(&self) -> Option<u64> {
        self.last_theirs
    }

    /// The round this side's next price would belong to.
    #[must_use]
    pub const fn next_round(&self) -> u8 {
        match self.own.side {
            Side::Sell if self.last_ours.is_some() => self.round + 1,
            _ => self.round,
        }
    }

    /// A price with no model involved, for when no model can be asked.
    ///
    /// Starts 2% to this side's favour of the market and moves 1% a round, stopping at
    /// the user's limit. On its last chance a side takes anything its limit allows, so
    /// two of these agree whenever the limits overlap. A user with no limit is treated
    /// as having one 5% off the market, so a hostile peer cannot walk them to nothing.
    #[must_use]
    pub fn fallback_price(&self, market_cents: u64) -> u64 {
        let next_round = self.next_round();
        let spread = u64::from(MAX_ROUNDS.saturating_sub(next_round));
        match self.own.side {
            Side::Sell => {
                let floor = self
                    .own
                    .limit_cents
                    .unwrap_or(percent(market_cents, 100 - NO_LIMIT_MARGIN_PERCENT));
                let ask = percent(market_cents, 100 + spread).max(floor);
                match self.last_theirs {
                    Some(theirs) if next_round > MAX_ROUNDS && theirs >= floor => theirs,
                    _ => ask,
                }
            }
            Side::Buy => {
                let ceiling = self
                    .own
                    .limit_cents
                    .unwrap_or(percent(market_cents, 100 + NO_LIMIT_MARGIN_PERCENT));
                let bid = percent(market_cents, 100 - spread).min(ceiling);
                match self.last_theirs {
                    Some(theirs) if next_round >= MAX_ROUNDS && theirs <= ceiling => theirs,
                    _ => bid,
                }
            }
        }
    }

    /// Whether `ours` is at least as generous as `theirs`, so asking for it would be
    /// asking for worse than what is already on the table.
    fn meets(&self, ours: u64, theirs: u64) -> bool {
        match self.own.side {
            Side::Buy => ours >= theirs,
            Side::Sell => ours <= theirs,
        }
    }

    fn on_accept(&mut self, price_cents: u64) -> Outcome {
        if let Some(mine) = self.accepted {
            if mine == price_cents {
                self.agreed = true;
                return Outcome::Agreed {
                    price_cents,
                    reply: None,
                };
            }
            self.ended = Some(EndReason::Unmatched);
            return Outcome::Ended {
                reason: EndReason::Unmatched,
            };
        }
        // Only this side's own last price, while it is waiting on an answer to it.
        if self.our_turn || self.last_ours != Some(price_cents) {
            return Outcome::Continue;
        }
        self.accepted = Some(price_cents);
        self.agreed = true;
        let reply = Msg::Accept {
            neg_id: self.neg_id.clone(),
            round: self.round,
            price_cents,
        };
        Outcome::Agreed {
            price_cents,
            reply: Some(reply),
        }
    }

    fn heard(&mut self, price_cents: u64, round: u8, now_ms: u64) {
        if price_cents == 0 {
            return;
        }
        self.round = round;
        self.last_theirs = Some(price_cents);
        self.our_turn = true;
        self.deadline_ms = now_ms.saturating_add(ROUND_TIMEOUT_MS);
    }

    fn spoke(&mut self, now_ms: u64) {
        self.our_turn = false;
        self.deadline_ms = now_ms.saturating_add(ROUND_TIMEOUT_MS);
    }
}

fn percent(value: u64, percent: u64) -> u64 {
    value.saturating_mul(percent) / 100
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seller(floor: Option<u64>) -> Negotiation {
        Negotiation::start(
            "n".into(),
            Bound {
                side: Side::Sell,
                limit_cents: floor,
            },
            0,
        )
    }

    fn buyer(ceiling: Option<u64>) -> Negotiation {
        Negotiation::start(
            "n".into(),
            Bound {
                side: Side::Buy,
                limit_cents: ceiling,
            },
            0,
        )
    }

    fn offer(round: u8, price_cents: u64) -> Msg {
        Msg::Offer {
            neg_id: "n".into(),
            round,
            price_cents,
        }
    }

    fn counter(round: u8, price_cents: u64) -> Msg {
        Msg::Counter {
            neg_id: "n".into(),
            round,
            price_cents,
        }
    }

    fn accept(round: u8, price_cents: u64) -> Msg {
        Msg::Accept {
            neg_id: "n".into(),
            round,
            price_cents,
        }
    }

    #[test]
    fn seller_cannot_propose_below_floor() {
        let mut n = seller(Some(390));
        assert_eq!(n.propose(389, 1), Err(Reject::OutsideBound));
    }

    #[test]
    fn buyer_cannot_propose_above_ceiling() {
        let mut n = buyer(Some(420));
        n.receive(&offer(1, 450), 1);
        assert_eq!(n.propose(421, 2), Err(Reject::OutsideBound));
        assert_eq!(n.propose(420, 3), Ok(counter(1, 420)));
    }

    #[test]
    fn a_price_of_zero_is_never_proposed() {
        let mut n = seller(None);
        assert_eq!(n.propose(0, 1), Err(Reject::OutsideBound));
    }

    #[test]
    fn messages_never_contain_the_bound() {
        let mut n = seller(Some(390));
        let msg = n.propose(450, 1).unwrap();
        assert!(!serde_json::to_string(&msg).unwrap().contains("390"));
    }

    #[test]
    fn the_seller_opens() {
        assert_eq!(seller(None).propose(450, 1), Ok(offer(1, 450)));
        assert_eq!(buyer(None).propose(400, 1), Err(Reject::NotOurTurn));
    }

    #[test]
    fn a_side_cannot_speak_twice_in_a_row() {
        let mut n = seller(None);
        n.propose(450, 1).unwrap();
        assert_eq!(n.propose(440, 2), Err(Reject::NotOurTurn));
    }

    #[test]
    fn fourth_round_is_rejected() {
        let mut n = seller(None);
        n.propose(450, 1).unwrap();
        n.receive(&counter(1, 400), 2);
        assert_eq!(n.propose(440, 3), Ok(counter(2, 440)));
        n.receive(&counter(2, 410), 4);
        assert_eq!(n.propose(430, 5), Ok(counter(3, 430)));
        n.receive(&counter(3, 415), 6);

        assert_eq!(n.propose(425, 7), Err(Reject::Ended));
        assert_eq!(n.ended(), Some(EndReason::RoundsExhausted));
    }

    #[test]
    fn the_seller_can_still_accept_after_the_last_round() {
        let mut n = seller(None);
        n.propose(450, 1).unwrap();
        n.receive(&counter(1, 400), 2);
        n.propose(440, 3).unwrap();
        n.receive(&counter(2, 410), 4);
        n.propose(430, 5).unwrap();
        n.receive(&counter(3, 415), 6);

        assert_eq!(n.propose(415, 7), Ok(accept(3, 415)));
    }

    #[test]
    fn a_counter_for_a_fourth_round_is_ignored() {
        let mut n = buyer(None);
        n.receive(&offer(1, 450), 1);
        n.propose(400, 2).unwrap();
        n.receive(&counter(2, 440), 3);
        n.propose(410, 4).unwrap();
        n.receive(&counter(3, 430), 5);
        n.propose(415, 6).unwrap();

        assert_eq!(n.receive(&counter(4, 420), 7), Outcome::Continue);
        assert_eq!(n.propose(420, 8), Err(Reject::NotOurTurn));
    }

    #[test]
    fn round_times_out_after_60s() {
        let mut n = seller(None);
        assert_eq!(n.tick(60_000), Outcome::Continue);
        assert_eq!(
            n.tick(60_001),
            Outcome::Ended {
                reason: EndReason::TimedOut
            }
        );
        assert_eq!(n.tick(60_002), Outcome::Continue, "ending is reported once");
        assert_eq!(n.propose(450, 60_003), Err(Reject::Ended));
    }

    #[test]
    fn each_message_restarts_the_clock() {
        let mut n = seller(None);
        n.propose(450, 50_000).unwrap();
        assert_eq!(n.tick(100_000), Outcome::Continue);
        n.receive(&counter(1, 400), 100_000);
        assert_eq!(n.tick(160_000), Outcome::Continue);
        assert_eq!(
            n.tick(160_001),
            Outcome::Ended {
                reason: EndReason::TimedOut
            }
        );
    }

    #[test]
    fn a_message_after_the_deadline_ends_it_rather_than_counting() {
        let mut n = seller(None);
        n.propose(450, 1).unwrap();
        let late = n.receive(&counter(1, 400), 60_002);
        assert_eq!(
            late,
            Outcome::Ended {
                reason: EndReason::TimedOut
            }
        );
    }

    #[test]
    fn accept_for_unoffered_price_is_ignored() {
        let mut n = seller(None);
        n.propose(450, 1).unwrap();
        assert_eq!(n.receive(&accept(1, 300), 2), Outcome::Continue);
        assert_eq!(n.ended(), None);
    }

    #[test]
    fn accept_for_an_earlier_price_is_ignored() {
        let mut n = seller(None);
        n.propose(450, 1).unwrap();
        n.receive(&counter(1, 400), 2);
        n.propose(440, 3).unwrap();
        assert_eq!(n.receive(&accept(2, 450), 4), Outcome::Continue);
    }

    #[test]
    fn accept_for_an_old_round_is_ignored() {
        let mut n = seller(None);
        n.propose(450, 1).unwrap();
        n.receive(&counter(1, 400), 2);
        n.propose(440, 3).unwrap();
        assert_eq!(n.receive(&accept(1, 440), 4), Outcome::Continue);
    }

    #[test]
    fn accept_after_end_is_ignored() {
        let mut n = seller(None);
        n.propose(450, 1).unwrap();
        n.tick(60_002);
        assert_eq!(n.receive(&accept(1, 450), 60_003), Outcome::Continue);
    }

    #[test]
    fn a_message_for_another_negotiation_is_ignored() {
        let mut n = seller(None);
        n.propose(450, 1).unwrap();
        let other = Msg::Accept {
            neg_id: "other".into(),
            round: 1,
            price_cents: 450,
        };
        assert_eq!(n.receive(&other, 2), Outcome::Continue);
    }

    #[test]
    fn matching_accepts_agree() {
        let mut s = seller(Some(390));
        let mut b = buyer(Some(420));

        let opening = s.propose(410, 1).unwrap();
        assert_eq!(b.receive(&opening, 2), Outcome::Continue);
        let acceptance = b.propose(410, 3).unwrap();
        assert_eq!(acceptance, accept(1, 410));

        let Outcome::Agreed {
            price_cents: 410,
            reply: Some(reply),
        } = s.receive(&acceptance, 4)
        else {
            panic!("the seller should agree and answer");
        };
        assert_eq!(
            b.receive(&reply, 5),
            Outcome::Agreed {
                price_cents: 410,
                reply: None
            }
        );
    }

    #[test]
    fn a_resent_accept_agrees_only_once() {
        let mut s = seller(None);
        s.propose(410, 1).unwrap();
        assert!(matches!(
            s.receive(&accept(1, 410), 2),
            Outcome::Agreed { .. }
        ));
        assert_eq!(s.receive(&accept(1, 410), 3), Outcome::Continue);
    }

    #[test]
    fn crossed_accepts_end_unmatched() {
        let mut b = buyer(None);
        b.receive(&offer(1, 410), 1);
        b.propose(410, 2).unwrap();
        assert_eq!(
            b.receive(&accept(1, 405), 3),
            Outcome::Ended {
                reason: EndReason::Unmatched
            }
        );
    }

    #[test]
    fn naming_a_better_price_than_the_other_side_accepts_theirs() {
        let mut b = buyer(None);
        b.receive(&offer(1, 400), 1);
        assert_eq!(
            b.propose(430, 2),
            Ok(accept(1, 400)),
            "a buyer never pays more than was asked"
        );

        let mut s = seller(None);
        s.propose(450, 1).unwrap();
        s.receive(&counter(1, 460), 2);
        assert_eq!(
            s.propose(440, 3),
            Ok(accept(1, 460)),
            "a seller never takes less than was bid"
        );
    }

    #[test]
    fn a_resent_offer_does_not_change_whose_turn_it_is() {
        let mut b = buyer(None);
        b.receive(&offer(1, 450), 1);
        b.propose(400, 2).unwrap();
        assert_eq!(b.receive(&offer(1, 450), 3), Outcome::Continue);
        assert_eq!(b.propose(405, 4), Err(Reject::NotOurTurn));
    }

    /// Runs both fallbacks against each other until something ends.
    fn play(floor: Option<u64>, ceiling: Option<u64>, market: u64) -> Option<u64> {
        let mut s = seller(floor);
        let mut b = buyer(ceiling);
        let mut now = 1;
        loop {
            let Ok(msg) = s.propose(s.fallback_price(market), now) else {
                return None;
            };
            now += 1;
            if let Outcome::Agreed {
                price_cents,
                reply: Some(reply),
            } = b.receive(&msg, now)
            {
                b.receive(&reply, now);
                return Some(price_cents);
            }
            let msg = b.propose(b.fallback_price(market), now).unwrap();
            now += 1;
            if let Outcome::Agreed {
                price_cents,
                reply: Some(reply),
            } = s.receive(&msg, now)
            {
                let Outcome::Agreed {
                    price_cents: seen, ..
                } = b.receive(&reply, now)
                else {
                    panic!("the buyer should agree too");
                };
                assert_eq!(seen, price_cents);
                return Some(price_cents);
            }
        }
    }

    #[test]
    fn fallback_agrees_near_the_market_when_limits_allow() {
        assert_eq!(play(Some(390), Some(420), 404), Some(404));
        assert_eq!(play(None, None, 404), Some(404));
    }

    #[test]
    fn fallback_agrees_whenever_the_limits_overlap_even_away_from_the_market() {
        for (floor, ceiling, market) in [
            (410, 420, 395),
            (380, 390, 404),
            (400, 400, 395),
            (400, 400, 500),
        ] {
            let price = play(Some(floor), Some(ceiling), market)
                .unwrap_or_else(|| panic!("{floor} {ceiling} {market}"));
            assert!(
                (floor..=ceiling).contains(&price),
                "{price} outside {floor}..{ceiling}"
            );
        }
    }

    #[test]
    fn fallback_finds_no_deal_when_the_limits_do_not_overlap() {
        assert_eq!(play(Some(420), Some(390), 404), None);
    }

    #[test]
    fn fallback_keeps_a_user_with_no_limit_near_the_market() {
        let mut s = seller(None);
        s.propose(s.fallback_price(400), 1).unwrap();
        s.receive(&counter(1, 10), 2);
        s.propose(s.fallback_price(400), 3).unwrap();
        s.receive(&counter(2, 10), 4);
        s.propose(s.fallback_price(400), 5).unwrap();
        s.receive(&counter(3, 10), 6);

        assert!(s.fallback_price(400) >= 380);
    }
}
