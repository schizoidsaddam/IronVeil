//! Procedural faction identity and event language.
//!
//! A faction's `FactionIdentity` is derived deterministically from its name seed.
//! It defines the faction's goal, voice (how they're described), tenets, and
//! the language used for events involving them.

use sha2::{Digest, Sha256};

// ── Faction goal taxonomy ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactionGoal {
    /// Accumulate wealth and control trade routes
    Commerce,
    /// Expand territory by force
    Conquest,
    /// Preserve ancient knowledge or traditions
    Preservation,
    /// Purge corruption or perceived evil
    Purity,
    /// Survive and protect their own people
    Survival,
    /// Restore a fallen empire, bloodline, or order
    Restoration,
    /// Control information and influence from the shadows
    Dominion,
    /// Seek transcendence, the divine, or forbidden knowledge
    Ascension,
}

impl FactionGoal {
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::Commerce     => "commerce",
            Self::Conquest     => "conquest",
            Self::Preservation => "preservation",
            Self::Purity       => "purity",
            Self::Survival     => "survival",
            Self::Restoration  => "restoration",
            Self::Dominion     => "dominion",
            Self::Ascension    => "ascension",
        }
    }

    fn from_u8(n: u8) -> Self {
        match n % 8 {
            0 => Self::Commerce,
            1 => Self::Conquest,
            2 => Self::Preservation,
            3 => Self::Purity,
            4 => Self::Survival,
            5 => Self::Restoration,
            6 => Self::Dominion,
            _ => Self::Ascension,
        }
    }
}

// ── Faction alignment ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactionAlignment {
    /// Open about their methods and goals
    Forthright,
    /// Secretive, operates through agents
    Shadow,
    /// Rigid, legalistic, rule-bound
    Ordered,
    /// Unpredictable, driven by zealotry or ideology
    Zealous,
    /// Pragmatic, will deal with anyone
    Opportunist,
}

impl FactionAlignment {
    fn from_u8(n: u8) -> Self {
        match n % 5 {
            0 => Self::Forthright,
            1 => Self::Shadow,
            2 => Self::Ordered,
            3 => Self::Zealous,
            _ => Self::Opportunist,
        }
    }
}

// ── Full identity ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FactionIdentity {
    pub goal:      FactionGoal,
    pub alignment: FactionAlignment,
    /// Short declaration of purpose used in founding event
    pub founding_declaration: &'static str,
    /// How skirmish victories are described for this faction
    pub victory_voice: &'static str,
    /// How collapse is described
    pub collapse_voice: &'static str,
    /// How civil war / restart is described
    pub civil_war_voice: &'static str,
    /// How interregnum / startup is described
    pub interregnum_voice: &'static str,
    /// How stabilization is described
    pub stable_voice: &'static str,
    /// Single tenet — what they hold above all else
    pub tenet: &'static str,
}

/// Derive a deterministic `FactionIdentity` from a faction name.
/// Same name always produces the same identity.
pub fn faction_identity(name: &str) -> FactionIdentity {
    let hash = Sha256::digest(name.as_bytes());
    let goal      = FactionGoal::from_u8(hash[0]);
    let alignment = FactionAlignment::from_u8(hash[1]);
    build_identity(goal, alignment, hash[2])
}

/// Derive identity from a container name (for docker-spawned factions).
pub fn faction_identity_from_container(container: &str) -> FactionIdentity {
    let hash = Sha256::digest(container.as_bytes());
    let goal      = FactionGoal::from_u8(hash[3]);
    let alignment = FactionAlignment::from_u8(hash[4]);
    build_identity(goal, alignment, hash[5])
}

fn build_identity(goal: FactionGoal, alignment: FactionAlignment, variant: u8) -> FactionIdentity {
    use FactionGoal::*;
    use FactionAlignment::*;

    let founding_declaration = match (goal, alignment) {
        (Commerce,     Forthright)  => "They have opened their ledgers to all who would trade.",
        (Commerce,     Shadow)      => "Their ships move without flags. Their coin speaks where names cannot.",
        (Commerce,     Ordered)     => "They have ratified their charter and established a weighing house.",
        (Commerce,     Zealous)     => "They believe the market is the truest expression of divine will.",
        (Commerce,     Opportunist) => "They trade in everything. Ask no questions about the provenance.",
        (Conquest,     Forthright)  => "They have planted their standard and named their borders.",
        (Conquest,     Shadow)      => "They do not march armies. They displace the weak until nothing remains.",
        (Conquest,     Ordered)     => "They have issued a formal declaration of territorial intent.",
        (Conquest,     Zealous)     => "They believe the land itself calls out to be claimed by the worthy.",
        (Conquest,     Opportunist) => "They take what isn't held. They hold what can be defended.",
        (Preservation, Forthright)  => "They have opened an archive and invited all scholars.",
        (Preservation, Shadow)      => "They collect what others discard, remembering what others forget.",
        (Preservation, Ordered)     => "They have established a registry of what must not be lost.",
        (Preservation, Zealous)     => "They would burn the world before letting a single text be destroyed.",
        (Preservation, Opportunist) => "They preserve only what is useful. The rest may rot.",
        (Purity,       Forthright)  => "They have announced their crusade and named their enemies openly.",
        (Purity,       Shadow)      => "They do not announce. They simply remove.",
        (Purity,       Ordered)     => "They have compiled their list of heresies and published it widely.",
        (Purity,       Zealous)     => "Their faith is a blade and they intend to use it.",
        (Purity,       Opportunist) => "What counts as corruption depends on who is asking.",
        (Survival,     Forthright)  => "They ask only to be left alone. They are prepared if they are not.",
        (Survival,     Shadow)      => "They do not advertise their presence. Surviving requires invisibility.",
        (Survival,     Ordered)     => "They have drawn up protocols for every catastrophe they can name.",
        (Survival,     Zealous)     => "They will sacrifice anything for continuity. Anyone.",
        (Survival,     Opportunist) => "Allies today. Alone tomorrow if necessary.",
        (Restoration,  Forthright)  => "They claim descent from a broken line and have published their genealogy.",
        (Restoration,  Shadow)      => "They work in the margins of history, restoring what was taken.",
        (Restoration,  Ordered)     => "They have a constitution. It predates most of the current kingdoms.",
        (Restoration,  Zealous)     => "The old ways were not merely better. They were sacred.",
        (Restoration,  Opportunist) => "They invoke the old names when convenient. Drop them when not.",
        (Dominion,     Forthright)  => "They have declared their intention to know everything that occurs.",
        (Dominion,     Shadow)      => "They have ears in every hall. Most of those halls do not know it.",
        (Dominion,     Ordered)     => "They have formalized their network of informants. There is paperwork.",
        (Dominion,     Zealous)     => "Knowledge is power. Power is rightfully theirs.",
        (Dominion,     Opportunist) => "They collect secrets the way others collect coin.",
        (Ascension,    Forthright)  => "They seek something beyond this world and make no secret of it.",
        (Ascension,    Shadow)      => "Their rituals occur in places no map records.",
        (Ascension,    Ordered)     => "They have codified the path to transcendence in seventeen volumes.",
        (Ascension,    Zealous)     => "The veil between worlds thins. They are why.",
        (Ascension,    Opportunist) => "They pursue the divine, but will pause for a profitable detour.",
    };

    let victory_voice: &'static str = match goal {
        Commerce     => select(variant, &["Their factors were first to the field.", "Gold followed the blood.", "They taxed the corpses."]),
        Conquest     => select(variant, &["Their banners now fly where others fell.", "The ground was claimed before the bodies cooled.", "Another border redrawn in their favor."]),
        Preservation => select(variant, &["They retrieved what would have been lost.", "The archive grows.", "They catalogued the battlefield before leaving."]),
        Purity       => select(variant, &["The corruption has been excised.", "Another enemy of the true order put down.", "They called it justice. It was not mercy."]),
        Survival     => select(variant, &["They endured. That is all they required.", "Another threat neutralized. They persist.", "They did not seek the fight. They finished it."]),
        Restoration  => select(variant, &["The old claim advances.", "Another step toward what was taken from them.", "History inches back toward what it should have been."]),
        Dominion     => select(variant, &["They know more now than they did before.", "The information was worth the blood.", "Another piece falls into their understanding."]),
        Ascension    => select(variant, &["Something watched the battle. It was pleased.", "The threshold draws closer.", "The rite was completed in the aftermath."]),
    };

    let collapse_voice: &'static str = match goal {
        Commerce     => "Their accounts are empty. Their creditors have moved on.",
        Conquest     => "Their armies dissolved. The land they took is already being divided.",
        Preservation => "The archive is gone. What they kept is now lost twice over.",
        Purity       => "The crusade devoured itself. The last of them burned each other.",
        Survival     => "They did not survive. The irony is noted in no record.",
        Restoration  => "What they sought to restore is further from reach than before.",
        Dominion     => "Their network collapsed. Secrets they held are now everyone's.",
        Ascension    => "Whatever they reached for did not hold them.",
    };

    let civil_war_voice: &'static str = match (goal, alignment) {
        (_, Zealous)     => "Schism. The true believers have turned on the insufficiently faithful.",
        (_, Ordered)     => "A procedural dispute has become a constitutional crisis.",
        (_, Shadow)      => "Someone talked. Now everyone knows. Now everyone suspects everyone.",
        (Commerce, _)    => "A debt dispute has become a blood feud.",
        (Conquest, _)    => "The generals could not agree on whose victory it was.",
        (Purity, _)      => "They began investigating each other for corruption.",
        (Dominion, _)    => "They turned their intelligence apparatus on themselves.",
        _                => "The internal contradictions have become external violence.",
    };

    let interregnum_voice: &'static str = match alignment {
        Forthright  => "Their leader is gone. Succession is openly contested.",
        Shadow      => "The center of their network has gone dark. Something is being decided quietly.",
        Ordered     => "The succession protocols have been invoked. The process will be followed.",
        Zealous     => "Their prophet is silent. The faithful wait for a sign.",
        Opportunist => "Leadership is vacant. Every ambitious member is currently very busy.",
    };

    let stable_voice: &'static str = match goal {
        Commerce     => "Their books are balanced. Trade resumes.",
        Conquest     => "Their ranks have closed. The advance will continue.",
        Preservation => "The collection is intact. Scholars return to their work.",
        Purity       => "The heretics have been dealt with. The faithful are unified again.",
        Survival     => "They have weathered it. They are still here.",
        Restoration  => "The old ways reassert themselves. The work continues.",
        Dominion     => "The network has been repaired. They are watching again.",
        Ascension    => "The rites resume. The veil is thin here.",
    };

    let tenet: &'static str = match goal {
        Commerce     => "What is not traded is wasted.",
        Conquest     => "Unclaimed land is an invitation.",
        Preservation => "To lose knowledge is to die twice.",
        Purity       => "Corruption is patient. So are we.",
        Survival     => "We do not require victory. We require continuity.",
        Restoration  => "What was taken can be reclaimed.",
        Dominion     => "To know is to control. To control is enough.",
        Ascension    => "The world is a threshold. We intend to cross it.",
    };

    FactionIdentity {
        goal,
        alignment,
        founding_declaration,
        victory_voice,
        collapse_voice,
        civil_war_voice,
        interregnum_voice,
        stable_voice,
        tenet,
    }
}

fn select(variant: u8, options: &[&'static str]) -> &'static str {
    options[variant as usize % options.len()]
}

// ── Skirmish language ────────────────────────────────────────────────────────

/// Generate a skirmish narrative from both factions' identities.
pub fn skirmish_entry(
    day:         i32,
    winner_name: &str,
    loser_name:  &str,
    winner_id:   &FactionIdentity,
    loser_id:    &FactionIdentity,
    variant:     u8,
) -> String {
    let scale = match variant % 3 {
        0 => "a skirmish",
        1 => "a border engagement",
        _ => "an armed confrontation",
    };

    let consequence = match loser_id.goal {
        FactionGoal::Commerce     => "Trade from their territory slows.",
        FactionGoal::Conquest     => "Their advance is checked, for now.",
        FactionGoal::Preservation => "What they were protecting is now at risk.",
        FactionGoal::Purity       => "Their crusade loses momentum.",
        FactionGoal::Survival     => "They retreat deeper and regroup.",
        FactionGoal::Restoration  => "The old claim weakens by one defeat.",
        FactionGoal::Dominion     => "Their agents withdraw. The silence is noted.",
        FactionGoal::Ascension    => "Their rites are interrupted. The threshold recedes.",
    };

    format!(
        "Day {day}. {scale} broke out between {} and {}. {} held. {} {}",
        winner_name, loser_name, winner_name,
        winner_id.victory_voice,
        consequence,
    )
}

/// Generate a warlord entry that reflects the faction they rose from.
pub fn warlord_entry(
    day:          i32,
    leader_name:  &str,
    faction_name: &str,
    identity:     &FactionIdentity,
) -> String {
    let context = match identity.goal {
        FactionGoal::Commerce     => "They emerged from the merchant disputes with leverage over everyone.",
        FactionGoal::Conquest     => "They united the fractured armies under a single brutal vision.",
        FactionGoal::Preservation => "They seized the archive and declared themselves its sole interpreter.",
        FactionGoal::Purity       => "They named themselves the final arbiter of who is corrupt.",
        FactionGoal::Survival     => "They promised survival. Many believed them.",
        FactionGoal::Restoration  => "They claim the bloodline. They may even be right.",
        FactionGoal::Dominion     => "They consolidated the intelligence network. Now they know everything.",
        FactionGoal::Ascension    => "Something spoke to them. They alone heard it clearly.",
    };

    format!(
        "Day {day}. From within {faction_name}, {leader_name} has risen. \
         {context} \
         Their tenet: \"{}\". They will not be easily forgotten.",
        identity.tenet,
    )
}

// ── Bandit/crisis event language ─────────────────────────────────────────────

pub fn bandit_event(day: i32, seed: u8) -> String {
    let events: &[&str] = &[
        "Caravans are being stopped on the eastern roads. Merchants report armed men with no colors.",
        "A toll has appeared on the northern pass. No one authorized it.",
        "Three merchant convoys failed to arrive this week. No wreckage has been found.",
        "Armed groups are moving through the lowlands. They are not soldiers. They are organized.",
        "Travelers report figures watching the roads from the treeline. No one has been stopped. Yet.",
        "The guilds are hiring guards. The rate has tripled in a week.",
    ];
    format!("Day {day}. {}", events[seed as usize % events.len()])
}

pub fn disaster_event(day: i32, seed: u8) -> String {
    let events: &[&str] = &[
        "A blight moves through the lowland crops. The granaries will not last the season.",
        "Three border lords have been found dead within a fortnight. No common enemy is named.",
        "The river has changed course. Three settlements are now unreachable.",
        "A fire of unknown origin destroyed a significant portion of the market district.",
        "A sickness moves through the soldier camps. The armies are weakened but not broken.",
        "An eclipse was observed for seventeen minutes. The interpretation is disputed. The fear is not.",
    ];
    format!("Day {day}. {}", events[seed as usize % events.len()])
}

pub fn rift_event(day: i32, swap_pct: f32, seed: u8) -> String {
    let events: &[&str] = &[
        "The geometry of a known road is wrong. Travelers arrive at destinations they did not intend.",
        "Something has been making sounds in the lower districts that no animal makes.",
        "Three scribes reported the same dream independently. They refuse to describe it.",
        "A structure has appeared that was not there yesterday. Its doors open inward onto nothing.",
        "The shadows in the old quarter have been falling at the wrong angle since nightfall.",
        "A scholar measuring the ley lines found that two of them now intersect where they did not.",
    ];
    format!(
        "Day {day}. The boundary thins ({swap_pct:.0}% pressure). {}",
        events[seed as usize % events.len()]
    )
}

pub fn earthquake_event(day: i32, seed: u8) -> String {
    let events: &[&str] = &[
        "The tunnels beneath the old quarter collapsed without warning. The sound carried for miles.",
        "A section of the city wall cracked along a line no one had mapped. It is not structural. Yet.",
        "Three wells went dry simultaneously. The ground is moving below them.",
        "The lower roads have subsided. Cart traffic has stopped. Walking is treacherous.",
        "A deep vibration was felt in the foundations. Dust fell from every ceiling in the district.",
        "The quarry to the north has opened a new fissure. The workers refuse to return.",
    ];
    format!("Day {day}. {}", events[seed as usize % events.len()])
}

// ── Cascade event language ────────────────────────────────────────────────────

pub fn revolt_event(day: i32, province: &str, seed: u8) -> String {
    let events: &[&str] = &[
        "The garrison was overwhelmed before dawn. By midday the gates were open to anyone.",
        "Someone burned the tax records. Someone else burned the collector. It spread from there.",
        "Three days of bread riots became something with a name and a direction.",
        "The lord was found in the morning. What remained of him was nailed to the granary door.",
        "They stopped asking for less and started taking it. No one has stopped them yet.",
    ];
    format!("Day {day}. {province} revolts. {}", events[seed as usize % events.len()])
}

pub fn famine_event(day: i32, province: &str, seed: u8) -> String {
    let events: &[&str] = &[
        "The last reserves have been counted. The number is wrong.",
        "Children are being sent to relatives in other provinces. Some provinces are sending them back.",
        "The market price for grain has exceeded what most families earn in a month.",
        "Three wells have gone brackish. The remaining one is guarded.",
    ];
    format!("Day {day}. Famine tightens in {province}. {}", events[seed as usize % events.len()])
}

pub fn war_declaration(day: i32, attacker: &str, defender: &str, attacker_id: &FactionIdentity) -> String {
    let reason = match attacker_id.goal {
        FactionGoal::Commerce     => "a trade dispute that has exhausted diplomatic channels",
        FactionGoal::Conquest     => "territorial ambition that no longer requires justification",
        FactionGoal::Preservation => "the destruction of something that cannot be replaced",
        FactionGoal::Purity       => "the unchecked spread of corruption into sacred ground",
        FactionGoal::Survival     => "an existential threat they can no longer ignore",
        FactionGoal::Restoration  => "a claim older than the current order",
        FactionGoal::Dominion     => "information that changed their calculus entirely",
        FactionGoal::Ascension    => "an omen that left no alternative",
    };
    format!(
        "Day {day}. {attacker} has declared war on {defender}. The cause: {reason}. \
         {}",
        attacker_id.victory_voice
    )
}

pub fn war_escalation(day: i32, attacker: &str, defender: &str, province: &str) -> String {
    format!(
        "Day {day}. The war between {attacker} and {defender} reaches {province}. \
         The province burns at the edges."
    )
}

pub fn rumor_event(day: i32, content: &str, accuracy: i64) -> String {
    if accuracy >= 80 {
        format!("Day {day}. A credible report: {content}")
    } else if accuracy >= 50 {
        format!("Day {day}. Word of mouth, reliability unclear: {content}")
    } else {
        format!("Day {day}. A garbled rumor, origin unknown: {content}")
    }
}

pub fn trade_disruption(day: i32, province_a: &str, province_b: &str, cause: &str) -> String {
    format!(
        "Day {day}. Trade between {province_a} and {province_b} has been disrupted. Cause: {cause}."
    )
}

pub fn power_vacuum(day: i32, province: &str, goal_faction: &str) -> String {
    format!(
        "Day {day}. With the collapse in {province}, {goal_faction} moves to fill the void. \
         They are not subtle about it."
    )
}

pub fn stability_collapse(day: i32, province: &str) -> String {
    format!(
        "Day {day}. {province} has lost the last of its coherent governance. \
         What happens there now happens without law."
    )
}
