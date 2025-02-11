use futures::StreamExt;
use irc::client::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;

mod poker {
    use rand::seq::SliceRandom;
    use rand::thread_rng;
    use std::collections::HashMap;
    use std::fmt;

    // ─── CARD DEFINITIONS ────────────────────────────────────────────────

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Suit {
        Clubs,
        Diamonds,
        Hearts,
        Spades,
    }

    impl fmt::Display for Suit {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let s = match self {
                Suit::Clubs => "♣",
                Suit::Diamonds => "♦",
                Suit::Hearts => "♥",
                Suit::Spades => "♠",
            };
            write!(f, "{}", s)
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Card {
        pub rank: u8, // 2-14 (where 11=J, 12=Q, 13=K, 14=A)
        pub suit: Suit,
    }

    impl fmt::Display for Card {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let rank_str = match self.rank {
                2..=10 => self.rank.to_string(),
                11 => "J".to_string(),
                12 => "Q".to_string(),
                13 => "K".to_string(),
                14 => "A".to_string(),
                _ => "?".to_string(),
            };
            write!(f, "{}{}", rank_str, self.suit)
        }
    }

    pub fn create_deck() -> Vec<Card> {
        let mut deck = Vec::new();
        let suits = [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];
        for &suit in &suits {
            for rank in 2..=14 {
                deck.push(Card { rank, suit });
            }
        }
        deck
    }

    pub fn shuffle_deck(deck: &mut Vec<Card>) {
        let mut rng = thread_rng();
        deck.shuffle(&mut rng);
    }

    // ─── BETTING ACTIONS AND GAME STAGES ──────────────────────────────────

    #[derive(Debug, PartialEq, Eq, Clone)]
    pub enum BetAction {
        Bet(u32),
        Check,
        Fold,
        Call,
    }

    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub enum RoundStage {
        PreFlop,
        Flop,
        Turn,
        River,
        Showdown,
    }

    impl fmt::Display for RoundStage {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let stage = match self {
                RoundStage::PreFlop => "PreFlop",
                RoundStage::Flop => "Flop",
                RoundStage::Turn => "Turn",
                RoundStage::River => "River",
                RoundStage::Showdown => "Showdown",
            };
            write!(f, "{}", stage)
        }
    }

    // ─── PLAYER AND GAME STATE STRUCTURES ───────────────────────────────

    #[derive(Debug)]
    pub struct Player {
        pub nick: String,
        pub chips: u32,
        pub hand: Vec<Card>,
        pub current_bet: u32,
        pub folded: bool,
    }

    impl Player {
        pub fn new(nick: &str) -> Self {
            Player {
                nick: nick.to_owned(),
                chips: 1000,
                hand: Vec::new(),
                current_bet: 0,
                folded: false,
            }
        }
    }

    #[derive(Debug)]
    pub struct PokerGame {
        pub players: Vec<Player>,
        pub deck: Vec<Card>,
        pub community_cards: Vec<Card>,
        pub pot: u32,
        pub current_bet: u32,
        pub current_turn: usize, // index into players
        pub stage: Option<RoundStage>, // None means lobby (no round in progress)
    }

    impl PokerGame {
        pub fn new() -> Self {
            PokerGame {
                players: Vec::new(),
                deck: Vec::new(),
                community_cards: Vec::new(),
                pot: 0,
                current_bet: 0,
                current_turn: 0,
                stage: None,
            }
        }

        /// Join the game (only allowed when no round is in progress)
        pub fn join(&mut self, nick: &str) -> String {
            if self.players.iter().any(|p| p.nick == nick) {
                return format!("{} is already in the game.", nick);
            }
            if self.stage.is_some() {
                return "A round is already in progress. Please wait for the next round.".to_string();
            }
            self.players.push(Player::new(nick));
            format!("{} joined the game.", nick)
        }

        /// Start a new round (requires at least 2 players)
        pub fn start_round(&mut self) -> String {
            if self.players.len() < 2 {
                return "Need at least 2 players to start.".to_string();
            }
            if self.stage.is_some() {
                return "Round already in progress.".to_string();
            }
            // (Re)initialize deck, community cards, pot, bets, etc.
            self.deck = create_deck();
            shuffle_deck(&mut self.deck);
            self.community_cards.clear();
            self.pot = 0;
            self.current_bet = 0;
            self.current_turn = 0;

            // Deal two cards to each player.
            for player in self.players.iter_mut() {
                player.hand.clear();
                player.folded = false;
                player.current_bet = 0;
                if self.deck.len() >= 2 {
                    player.hand.push(self.deck.pop().unwrap());
                    player.hand.push(self.deck.pop().unwrap());
                }
            }
            self.stage = Some(RoundStage::PreFlop);

            // In a real bot you might send each player a private message with their hand.
            let mut msg = String::from("Round started! Dealing hole cards.\n");
            for player in &self.players {
                msg.push_str(&format!(
                    "{}'s hand: {} {}\n",
                    player.nick, player.hand[0], player.hand[1]
                ));
            }
            msg.push_str("PreFlop betting round begins. ");
            if let Some(p) = self.players.get(self.current_turn) {
                msg.push_str(&format!("It's {}'s turn.", p.nick));
            }
            msg
        }

        /// Process a betting action (only when a round is in progress)
        pub fn process_bet(&mut self, nick: &str, action: BetAction) -> String {
            if self.stage.is_none() {
                return "No round in progress. Use !start to begin.".to_string();
            }
            // Ensure it is the acting player's turn.
            if self.players[self.current_turn].nick != nick {
                return format!("It's not your turn, {}.", nick);
            }
            let player = &mut self.players[self.current_turn];
            if player.folded {
                return format!("{}, you have already folded.", nick);
            }
            match action {
                BetAction::Fold => {
                    player.folded = true;
                    let msg = format!("{} folds.", nick);
                    self.advance_turn();
                    msg
                }
                BetAction::Bet(amount) => {
                    // For simplicity: if no bet yet, any bet is allowed; otherwise the bet must at least match.
                    if self.current_bet == 0 {
                        if amount > player.chips {
                            return format!("{} does not have enough chips.", nick);
                        }
                        player.chips -= amount;
                        player.current_bet = amount;
                        self.pot += amount;
                        self.current_bet = amount;
                        let msg = format!("{} bets {}.", nick, amount);
                        self.advance_turn();
                        msg
                    } else {
                        if amount < self.current_bet {
                            return format!("You must bet at least {}.", self.current_bet);
                        }
                        if amount > player.chips {
                            return format!("{} does not have enough chips.", nick);
                        }
                        player.chips -= amount;
                        player.current_bet = amount;
                        self.pot += amount;
                        let msg = format!("{} bets {}.", nick, amount);
                        self.advance_turn();
                        msg
                    }
                }
                BetAction::Call => {
                    let call_amount = self.current_bet.saturating_sub(player.current_bet);
                    if call_amount > player.chips {
                        return format!("{} does not have enough chips to call.", nick);
                    }
                    player.chips -= call_amount;
                    player.current_bet += call_amount;
                    self.pot += call_amount;
                    let msg = format!("{} calls, betting {}.", nick, call_amount);
                    self.advance_turn();
                    msg
                }
                BetAction::Check => {
                    if self.current_bet != 0 && player.current_bet < self.current_bet {
                        return format!(
                            "Cannot check, you need to call or bet {}.",
                            self.current_bet
                        );
                    }
                    let msg = format!("{} checks.", nick);
                    self.advance_turn();
                    msg
                }
            }
        }

        /// Advance the turn pointer to the next active (non-folded) player.
        fn advance_turn(&mut self) {
            let n = self.players.len();
            // This very simplified version just moves to the next player.
            for i in 1..=n {
                let next = (self.current_turn + i) % n;
                if !self.players[next].folded {
                    self.current_turn = next;
                    return;
                }
            }
            // If no active players remain, do nothing.
        }

        /// Advance the game stage (from PreFlop → Flop → Turn → River → Showdown)
        /// In this simplified bot you must use the !next command to move the game forward.
        pub fn advance_stage(&mut self) -> String {
            // Before moving on, reset players’ current bets.
            for player in self.players.iter_mut() {
                player.current_bet = 0;
            }
            self.current_bet = 0;
            match self.stage {
                Some(RoundStage::PreFlop) => {
                    // Deal the Flop (3 community cards)
                    if self.deck.len() >= 3 {
                        self.community_cards.clear();
                        self.community_cards.push(self.deck.pop().unwrap());
                        self.community_cards.push(self.deck.pop().unwrap());
                        self.community_cards.push(self.deck.pop().unwrap());
                    }
                    self.stage = Some(RoundStage::Flop);
                    self.current_turn = 0;
                    while self.current_turn < self.players.len() && self.players[self.current_turn].folded {
                        self.current_turn += 1;
                    }
                    format!(
                        "Flop: {} {} {}\nFlop betting round begins. It's {}'s turn.",
                        self.community_cards[0],
                        self.community_cards[1],
                        self.community_cards[2],
                        self.players[self.current_turn].nick
                    )
                }
                Some(RoundStage::Flop) => {
                    // Deal the Turn (1 card)
                    if self.deck.len() >= 1 {
                        self.community_cards.push(self.deck.pop().unwrap());
                    }
                    self.stage = Some(RoundStage::Turn);
                    self.current_turn = 0;
                    while self.current_turn < self.players.len() && self.players[self.current_turn].folded {
                        self.current_turn += 1;
                    }
                    format!(
                        "Turn: {}\nTurn betting round begins. It's {}'s turn.",
                        self.community_cards.last().unwrap(),
                        self.players[self.current_turn].nick
                    )
                }
                Some(RoundStage::Turn) => {
                    // Deal the River (1 card)
                    if self.deck.len() >= 1 {
                        self.community_cards.push(self.deck.pop().unwrap());
                    }
                    self.stage = Some(RoundStage::River);
                    self.current_turn = 0;
                    while self.current_turn < self.players.len() && self.players[self.current_turn].folded {
                        self.current_turn += 1;
                    }
                    format!(
                        "River: {}\nRiver betting round begins. It's {}'s turn.",
                        self.community_cards.last().unwrap(),
                        self.players[self.current_turn].nick
                    )
                }
                Some(RoundStage::River) => {
                    // Move to showdown
                    self.stage = Some(RoundStage::Showdown);
                    let result = self.showdown();
                    // Reset stage to allow a new round.
                    self.stage = None;
                    result
                }
                _ => "Round already ended.".to_string(),
            }
        }

        /// At showdown, evaluate each active (non-folded) player’s hand and determine a winner.
        pub fn showdown(&self) -> String {
            let mut results = Vec::new();
            for player in &self.players {
                if !player.folded {
                    let best = evaluate_best_hand(&player.hand, &self.community_cards);
                    results.push((player.nick.clone(), best));
                }
            }
            if results.is_empty() {
                return "All players folded. No showdown.".to_string();
            }
            // Sort descending by hand rank.
            results.sort_by(|a, b| b.1.cmp(&a.1));
            let winner = &results[0];
            let mut msg = String::new();
            msg.push_str("Showdown results:\n");
            for (nick, hand_rank) in &results {
                msg.push_str(&format!("{}: {}\n", nick, hand_rank));
            }
            msg.push_str(&format!(
                "Winner: {} wins the pot of {} chips!",
                winner.0, self.pot
            ));
            msg
        }
    }

    // ─── HAND EVALUATION (Simplified) ────────────────────────────────────

    /// A HandRank consists of a category (higher is better) plus tie‐breaker values.
    /// Categories (by number): 9 = Straight Flush, 8 = Four of a Kind, 7 = Full House,
    /// 6 = Flush, 5 = Straight, 4 = Three of a Kind, 3 = Two Pair, 2 = One Pair, 1 = High Card.
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct HandRank(pub u8, pub Vec<u8>);

    impl fmt::Display for HandRank {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let description = match self.0 {
                9 => "Straight Flush",
                8 => "Four of a Kind",
                7 => "Full House",
                6 => "Flush",
                5 => "Straight",
                4 => "Three of a Kind",
                3 => "Two Pair",
                2 => "One Pair",
                1 => "High Card",
                _ => "Unknown",
            };
            write!(f, "{} ({:?})", description, self.1)
        }
    }

    /// Evaluate the best five-card hand using the hole cards and community cards.
    /// (This implementation iterates over all 5-card combinations of the 7 available cards.)
    pub fn evaluate_best_hand(hole_cards: &[Card], community_cards: &[Card]) -> HandRank {
        let mut all_cards = Vec::new();
        all_cards.extend_from_slice(hole_cards);
        all_cards.extend_from_slice(community_cards);
        let combinations = get_combinations(&all_cards, 5);
        let mut best = HandRank(0, vec![]);
        for combo in combinations {
            let rank = evaluate_five_card_hand(&combo);
            if rank > best {
                best = rank;
            }
        }
        best
    }

    /// Evaluate a five-card hand. (This is a simplified evaluator.)
    fn evaluate_five_card_hand(cards: &[Card]) -> HandRank {
        // Get ranks and determine flush/straight status.
        let mut ranks: Vec<u8> = cards.iter().map(|c| c.rank).collect();
        ranks.sort_unstable_by(|a, b| b.cmp(a)); // descending order

        let is_flush = cards.iter().all(|c| c.suit == cards[0].suit);

        // Check for straight:
        let mut unique = ranks.clone();
        unique.sort_unstable();
        unique.dedup();
        let is_straight = if unique.len() >= 5 {
            let mut found = false;
            for window in unique.windows(5) {
                if window[4] - window[0] == 4 && window.windows(2).all(|w| w[1] - w[0] == 1) {
                    found = true;
                    break;
                }
            }
            // Special case: Ace low straight (A,2,3,4,5)
            if !found && unique.contains(&14) && unique.contains(&2) && unique.contains(&3) && unique.contains(&4) && unique.contains(&5) {
                found = true;
            }
            found
        } else {
            false
        };

        if is_flush && is_straight {
            // For simplicity, use the highest card as tie-breaker.
            return HandRank(9, vec![*ranks.first().unwrap()]);
        }

        // Count the frequency of each rank.
        let mut freq: HashMap<u8, u8> = HashMap::new();
        for &r in &ranks {
            *freq.entry(r).or_insert(0) += 1;
        }
        let mut counts: Vec<(u8, u8)> = freq.into_iter().collect(); // (rank, count)
        counts.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));

        if counts[0].1 == 4 {
            // Four of a Kind
            let kicker = ranks.iter().find(|&&r| r != counts[0].0).cloned().unwrap_or(0);
            return HandRank(8, vec![counts[0].0, kicker]);
        }
        if counts[0].1 == 3 && counts.len() > 1 && counts[1].1 >= 2 {
            // Full House
            return HandRank(7, vec![counts[0].0, counts[1].0]);
        }
        if is_flush {
            return HandRank(6, ranks.clone());
        }
        if is_straight {
            return HandRank(5, vec![*ranks.first().unwrap()]);
        }
        if counts[0].1 == 3 {
            // Three of a Kind
            let mut kickers: Vec<u8> = ranks.iter().cloned().filter(|&r| r != counts[0].0).collect();
            kickers.sort_unstable_by(|a, b| b.cmp(a));
            let mut tiebreakers = vec![counts[0].0];
            tiebreakers.extend(kickers.into_iter().take(2));
            return HandRank(4, tiebreakers);
        }
        if counts[0].1 == 2 && counts.len() > 1 && counts[1].1 == 2 {
            // Two Pair
            let pair1 = counts[0].0;
            let pair2 = counts[1].0;
            let kicker = ranks
                .iter()
                .find(|&&r| r != pair1 && r != pair2)
                .cloned()
                .unwrap_or(0);
            let mut pairs = vec![pair1, pair2];
            pairs.sort_unstable_by(|a, b| b.cmp(a));
            return HandRank(3, vec![pairs[0], pairs[1], kicker]);
        }
        if counts[0].1 == 2 {
            // One Pair
            let mut kickers: Vec<u8> = ranks.iter().cloned().filter(|&r| r != counts[0].0).collect();
            kickers.sort_unstable_by(|a, b| b.cmp(a));
            let mut tiebreakers = vec![counts[0].0];
            tiebreakers.extend(kickers.into_iter().take(3));
            return HandRank(2, tiebreakers);
        }
        // Otherwise, High Card.
        HandRank(1, ranks)
    }

    /// Generate all k–combinations of the given cards.
    fn get_combinations(cards: &[Card], k: usize) -> Vec<Vec<Card>> {
        let mut result = Vec::new();
        let n = cards.len();
        let mut indices: Vec<usize> = (0..k).collect();
        loop {
            let combo: Vec<Card> = indices.iter().map(|&i| cards[i]).collect();
            result.push(combo);
            // Generate next combination.
            let mut i = k;
            while i > 0 {
                i -= 1;
                if indices[i] != i + n - k {
                    break;
                }
            }
            if indices[0] == n - k {
                break;
            }
            indices[i] += 1;
            for j in i + 1..k {
                indices[j] = indices[j - 1] + 1;
            }
        }
        result
    }
}

use poker::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up an IRC configuration.
    // (Adjust the server, port, channel, and nickname as needed.)
    let config = Config {
        nickname: Some("PokerBot".to_owned()),
        server: Some("irc.example.com".to_owned()),
        channels: vec!["#poker".to_owned()],
        port: Some(6667),
        use_tls: Some(false),
        ..Default::default()
    };

    let mut client = Client::from_config(config).await?;
    client.identify()?;

    // Shared game state wrapped in a Tokio mutex.
    let game = Arc::new(Mutex::new(PokerGame::new()));
    println!("Connected to IRC. Waiting for messages...");

    let mut stream = client.stream()?;
    while let Some(message) = stream.next().await {
        match message {
            Ok(message) => {
                if let Command::PRIVMSG(ref target, ref msg) = message.command {
                    if let Some(nick) = message.source_nickname() {
                        // Only process commands that begin with an exclamation mark.
                        if msg.starts_with("!") {
                            if let Some(response) =
                                handle_command(&client, &game, nick, target, msg).await
                            {
                                // Send response back to the channel.
                                client.send_privmsg(target, response)?;
                            }
                        }
                    }
                }
            }
            Err(e) => println!("Error: {:?}", e),
        }
    }
    Ok(())
}

/// Parse and handle commands received from IRC.
async fn handle_command(
    _client: &Client,
    game: &Arc<Mutex<PokerGame>>,
    nick: &str,
    _target: &str,
    msg: &str,
) -> Option<String> {
    let tokens: Vec<&str> = msg.trim().split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    let cmd = tokens[0];
    let mut game = game.lock().await;
    let response = match cmd {
        "!join" => game.join(nick),
        "!start" => game.start_round(),
        "!bet" => {
            if tokens.len() < 2 {
                "Usage: !bet <amount>".to_string()
            } else if let Ok(amount) = tokens[1].parse::<u32>() {
                game.process_bet(nick, BetAction::Bet(amount))
            } else {
                "Invalid bet amount.".to_string()
            }
        }
        "!call" => game.process_bet(nick, BetAction::Call),
        "!check" => game.process_bet(nick, BetAction::Check),
        "!fold" => game.process_bet(nick, BetAction::Fold),
        // Use !next to advance the round (e.g. from PreFlop to Flop, etc.)
        "!next" => game.advance_stage(),
        _ => return None,
    };
    Some(response)
}

