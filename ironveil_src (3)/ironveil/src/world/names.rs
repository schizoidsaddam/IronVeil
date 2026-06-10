use rand::{Rng, SeedableRng};
use rand::rngs::SmallRng;

const ONSET: &[&str] = &[
    "Vor","Ash","Drev","Cal","Mor","Sel","Brak","Eld","Thal","Orm","Vyss","Kor",
    "Nev","Ul","Shan","Gren","Wyr","Fael","Hal","Dun","Crag","Iron","Bael",
];
const NUCLEUS: &[&str] = &[
    "en","orn","el","ath","an","or","al","um","em","ir","ur","eth","is","un",
];
const CODA: &[&str] = &[
    "moor","hold","fell","heim","veil","watch","fen","tor","reach","gate","keep","vale",
    "mark","stead","haven","cross","ford","wick","mere","dusk",
];
const TITLES: &[&str] = &[
    "the Ashen","the Grey","the Iron","the Pale","of the Fell","Blackhand",
    "Stoneheart","the Elder","the Younger","of Dusk","the Scarred","the Silent",
];
const ARCHETYPES: &[&str] = &[
    "guild","order","compact","covenant","brotherhood",
    "conclave","circle","syndicate","assembly","league",
];

pub fn province_name(seed: u64) -> String {
    let mut rng = SmallRng::seed_from_u64(seed);
    format!(
        "{}{}{}",
        ONSET[rng.gen_range(0..ONSET.len())],
        NUCLEUS[rng.gen_range(0..NUCLEUS.len())],
        CODA[rng.gen_range(0..CODA.len())],
    )
}

pub fn npc_name(seed: u64) -> String {
    let mut rng = SmallRng::seed_from_u64(seed);
    let first  = ONSET[rng.gen_range(0..ONSET.len())];
    let middle = NUCLEUS[rng.gen_range(0..NUCLEUS.len())];
    let suffix = if rng.gen_bool(0.4) {
        format!(" {}", TITLES[rng.gen_range(0..TITLES.len())])
    } else {
        String::new()
    };
    format!("{first}{middle}{suffix}")
}

/// Derive a faction name from a container name without being literal about it.
pub fn faction_name_from_container(container: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(container.as_bytes());
    let seed = u64::from_le_bytes(hash[..8].try_into().unwrap());
    let mut rng = SmallRng::seed_from_u64(seed);
    format!(
        "The {} {}",
        ONSET[rng.gen_range(0..ONSET.len())],
        title_case(CODA[rng.gen_range(0..CODA.len())]),
    )
}

pub fn faction_archetype_from_container(container: &str) -> &'static str {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(container.as_bytes());
    ARCHETYPES[hash[8] as usize % ARCHETYPES.len()]
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None    => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
