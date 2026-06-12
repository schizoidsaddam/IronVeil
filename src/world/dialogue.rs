//! Context-aware NPC dialogue and environmental text.
//!
//! All dialogue is grounded in actual world state passed in as parameters.
//! No generic fallbacks — every line reflects something real about this
//! province, this faction, this day.

/// Contextual world state passed to dialogue generators.
pub struct WorldContext<'a> {
    pub day:           i32,
    pub tension_label: &'a str,
    pub province_name: &'a str,
    pub stability:     i64,
    pub famine:        i64,
    pub revolt_risk:   i64,
    pub faction_name:  Option<&'a str>,
    pub faction_tenet: Option<&'a str>,
    pub at_war:        bool,
    pub war_attacker:  Option<&'a str>,
    pub war_defender:  Option<&'a str>,
    pub warlord_name:  Option<&'a str>,
    pub recent_event:  Option<&'a str>, // last codex entry for this province
}

impl<'a> WorldContext<'a> {
    pub fn blank(day: i32, province_name: &'a str) -> Self {
        Self {
            day, tension_label: "PEACEFUL", province_name,
            stability: 60, famine: 0, revolt_risk: 0,
            faction_name: None, faction_tenet: None,
            at_war: false, war_attacker: None, war_defender: None,
            warlord_name: None, recent_event: None,
        }
    }
}

/// Generate dialogue for an NPC given full world context.
/// Returns 1–3 lines the NPC might say, in order.
pub fn npc_lines(name: &str, role: &str, hostile: bool, ctx: &WorldContext) -> Vec<String> {
    if hostile {
        return hostile_lines(name, role, ctx);
    }

    let mut lines: Vec<String> = Vec::new();
    let seed = name.len() ^ (ctx.day as usize & 0xFF);

    match role {
        "merchant" => {
            if ctx.famine > 60 {
                lines.push(format!("\"The last shipment from {} never arrived.\"", ctx.province_name));
                lines.push("\"I'm selling at a loss just to clear out before things get worse.\"".into());
            } else if ctx.at_war {
                let att = ctx.war_attacker.unwrap_or("someone");
                let def = ctx.war_defender.unwrap_or("someone else");
                lines.push(format!("\"Neither {att} nor {def} pays for what they take.\""));
                lines.push("\"I don't ask whose coin it is. I just count it.\"".into());
            } else if ctx.stability < 40 {
                lines.push("\"The buyers have left. I'm not sure I blame them.\"".into());
                lines.push("\"I'll be gone before the next season. Mark that.\"".into());
            } else {
                let opts = ["\"Business is honest work. The only kind left.\"",
                            "\"You're the third stranger this week. Something is moving out there.\"",
                            "\"I trade in information as much as goods. Both are scarce.\""];
                lines.push(opts[seed % opts.len()].to_string());
            }
        }

        "lord" => {
            if let Some(warlord) = ctx.warlord_name {
                lines.push(format!("\"You've heard of {warlord}. Everyone has.\""));
                lines.push("\"My legitimacy is a polite fiction at this point.\"".into());
            } else if ctx.at_war {
                lines.push(format!("\"I hold {} by will alone. The walls help.\"", ctx.province_name));
                if let Some(t) = ctx.faction_tenet {
                    lines.push(format!("\"My people believe this: '{t}'. So do I.\""));
                }
            } else if ctx.revolt_risk > 60 {
                lines.push("\"They haven't moved against me yet. I emphasize 'yet'.\"".into());
                lines.push(format!("\"Three of my lords stopped answering messages. {}: a bad province to govern.\"",
                    ctx.province_name));
            } else {
                let opts = [
                    "\"The old compact is broken. What holds now is habit.\"",
                    "\"I received a letter this week. I haven't opened it.\"",
                    "\"My grandfather held this ground. I intend to as well.\"",
                ];
                lines.push(opts[seed % opts.len()].to_string());
            }
            if let Some(ev) = ctx.recent_event {
                if ev.len() < 80 {
                    lines.push(format!("\"You've perhaps heard: {ev}\""));
                }
            }
        }

        "soldier" => {
            if ctx.at_war {
                let att = ctx.war_attacker.unwrap_or("them");
                lines.push(format!("\"We've held this line against {att} for {n} days.\"",
                    n = ctx.day % 12 + 2));
                lines.push("\"I stopped counting the dead. The number stopped meaning anything.\"".into());
            } else if ctx.stability < 35 {
                lines.push("\"My orders haven't changed. The situation has.\"".into());
                lines.push("\"I enforce what I can. The rest I look past.\"".into());
            } else if ctx.tension_label == "CRISIS" {
                lines.push("\"Double watch tonight. No one said why.\"".into());
                lines.push("\"The captain is nervous. That makes me nervous.\"".into());
            } else {
                let opts = [
                    "\"Nothing moves on the road that I don't see.\"",
                    "\"I've been posted here three months. Feels longer.\"",
                    "\"Quiet duty is good duty. Remember that.\"",
                ];
                lines.push(opts[seed % opts.len()].to_string());
            }
        }

        "healer" => {
            if ctx.famine > 50 {
                lines.push("\"Malnutrition mostly. And the despair that follows it.\"".into());
                lines.push("\"I'm out of willowbark. Have been for a week.\"".into());
            } else if ctx.at_war {
                lines.push("\"They bring me soldiers from both sides. I don't ask which.\"".into());
                lines.push("\"I have not slept properly in eleven days.\"".into());
            } else {
                let opts = [
                    "\"Rest is the best medicine. You look like you need it.\"",
                    "\"I've seen worse than this. That's not a comfort.\"",
                    "\"The body knows things the mind refuses.\"",
                    "\"I charge what people can pay. Sometimes that's nothing.\"",
                ];
                lines.push(opts[seed % opts.len()].to_string());
            }
        }

        "scholar" => {
            if let Some(ev) = ctx.recent_event {
                if ev.len() < 100 {
                    lines.push(format!("\"I've been documenting what happened here. The record shows: '{ev}'\""));
                }
            }
            if ctx.stability < 30 {
                lines.push("\"I hid the archive when the fighting started. Most of it, anyway.\"".into());
            } else {
                let opts = [
                    "\"The chronicle is always incomplete. That's the point.\"",
                    "\"History doesn't care about accuracy. It cares about survival.\"",
                    "\"I've been here long enough to know what this province forgets about itself.\"",
                    "\"Knowledge costs more than coin in a place like this.\"",
                    "\"What do you want to know? More importantly — why?\"",
                ];
                lines.push(opts[seed % opts.len()].to_string());
            }
        }

        "bard" => {
            if ctx.at_war {
                lines.push(format!("\"There's a song starting about this war. Everyone's in it whether they like it or not.\""));
            }
            if ctx.stability < 35 {
                lines.push("\"The stories coming out of here are extraordinary. I wish they weren't.\"".into());
            } else {
                let opts = [
                    "\"I collect what people remember. Memory is the first thing to lie.\"",
                    "\"There are three versions of what happened here. All of them are true.\"",
                    "\"I've been to worse places. I've written better songs about them.\"",
                    "\"The chronicles miss most of it. I try to catch what falls through.\"",
                    "\"Your story isn't finished yet. I can tell by looking.\"",
                ];
                lines.push(opts[seed % opts.len()].to_string());
            }
        }

        "priest" => {
            if ctx.revolt_risk > 70 {
                lines.push("\"I've been hearing confessions all week. The content is consistent.\"".into());
            }
            if ctx.famine > 60 {
                lines.push("\"The old prayers don't specify what to do when the granary is empty. I improvise.\"".into());
            } else {
                let opts = [
                    "\"The old names still answer. Slowly, and not always helpfully.\"",
                    "\"Pray if you're inclined. It costs nothing and occasionally works.\"",
                    "\"Something watches from the rifts. I can't say whether it's benevolent.\"",
                    "\"My congregation is smaller than it was. Some left. Some are gone differently.\"",
                    "\"Faith is easier to maintain before things go wrong. We're past that point.\"",
                ];
                lines.push(opts[seed % opts.len()].to_string());
            }
        }

        "warlord" => {
            if let Some(tenet) = ctx.faction_tenet {
                lines.push(format!("\"I'll tell you what I believe: '{tenet}' Remember it.\""));
            }
            if let Some(fname) = ctx.faction_name {
                lines.push(format!("\"Everything here answers to {fname}. Everything here answers to me.\""));
            }
            lines.push("\"You're either useful or you're not. Decide quickly.\"".into());
        }

        "assassin" => {
            let opts = [
                "\"I'm between contracts.\"",
                "\"You didn't see me.\"",
                "\"I charge more for discretion than for the work itself.\"",
                "\"The interesting question isn't who. It's who's paying.\"",
            ];
            lines.push(opts[seed % opts.len()].to_string());
        }

        "blacksmith" => {
            if ctx.at_war {
                lines.push("\"I haven't slept in three days. The orders don't stop.\"".into());
                lines.push("\"I don't make weapons. I make tools. What people do with them isn't my business.\"".into());
            } else {
                let opts = [
                    "\"The iron's getting harder to source. The roads aren't safe.\"",
                    "\"Good work takes time. Time is the one thing nobody has.\"",
                    "\"I've repaired more than I've made new, lately. That tells you something.\"",
                ];
                lines.push(opts[seed % opts.len()].to_string());
            }
        }

        "thief" => {
            let opts = [
                "\"I find things. Sometimes they're already lost.\"",
                "\"The garrison has a gap in the north watch. Not that you'd care.\"",
                "\"I'm not stealing. I'm redistributing.\"",
                "\"I know who's left this province. And what they left behind.\"",
            ];
            lines.push(opts[seed % opts.len()].to_string());
        }

        "wanderer" => {
            if ctx.at_war {
                lines.push(format!("\"I came through {} three days ago. It looked different then.\"",
                    ctx.province_name));
            }
            let opts = [
                "\"I've been moving for six months. The world doesn't stop changing while you walk.\"",
                "\"I've seen what's north of here. Keep going south.\"",
                "\"Every road leads somewhere worse. At least the walking is honest.\"",
                "\"I don't carry much. I've learned what matters.\"",
            ];
            lines.push(opts[seed % opts.len()].to_string());
        }

        _ => {
            if ctx.stability < 30 {
                lines.push("\"I just want this to be over.\"".into());
            } else if ctx.famine > 55 {
                lines.push("\"I sent my family east last week. Felt right.\"".into());
            } else {
                let opts = [
                    "\"Uncertain times. You get used to it, or you leave.\"",
                    "\"I've nothing to say to strangers.\"",
                    "\"Keep your head down. That's all I know.\"",
                ];
                lines.push(opts[seed % opts.len()].to_string());
            }
        }
    }

    lines
}

fn hostile_lines(name: &str, role: &str, ctx: &WorldContext) -> Vec<String> {
    let seed = name.len() ^ role.len();
    match role {
        "warlord" => vec![
            format!("\"I am {name}. You have made a poor choice.\""),
            "\"This is the last conversation you'll have.\"".into(),
        ],
        "soldier" | "assassin" => {
            let faction = ctx.faction_name.unwrap_or("the compact");
            let by_order = format!("\"By order of {faction}—\"");
            let opts: &[&str] = &[
                "\"You shouldn't be here.\"",
                "\"This ends now.\"",
                "\"No witnesses.\"",
                &by_order,
            ];
            vec![opts[seed % opts.len()].to_string()]
        }
        _ => vec!["\"Back away.\"".to_string()],
    }
}

// ── Environmental text ────────────────────────────────────────────────────────

/// Returns ambient detail lines for a given biome + stability.
/// Called when the player moves into a new cell — surfaces texture.
pub fn ambient_cell(biome: &str, stability: i64, day: i32, seed: u32) -> Option<&'static str> {
    let s = seed as usize ^ (day as usize & 0xFF);

    // Stability-driven universal details
    if stability < 20 {
        let lines: &[&str] = &[
            "A burned-out doorframe. The hinges are still warm.",
            "Someone wrote a name on the wall and crossed it out.",
            "The floor is covered in ash and boot prints going one direction.",
            "A child's shoe, alone, in the middle of the corridor.",
            "Three bodies, old enough to not smell anymore.",
            "A barricade, broken from the inside.",
            "Blood on the floor, dried brown. Old enough to walk past.",
        ];
        return Some(lines[s % lines.len()]);
    }

    if stability < 40 {
        let lines: &[&str] = &[
            "Broken crockery swept into the corner. Someone tried to clean up.",
            "A notice pinned to the wall, half-torn. You read the half that's left.",
            "Furniture overturned and not righted. This happened fast.",
            "A lantern with no oil, hung where it's always hung.",
            "Someone scratched a tally into the wall. You don't know what it counts.",
            "The smell of smoke, old but present.",
        ];
        return Some(lines[s % lines.len()]);
    }

    // Biome-specific details
    let biome_lines: &[&str] = match biome {
        "forest" | "swamp" => &[
            "Moss growing in the cracks of the stone floor.",
            "The walls are damp. Water finds a way.",
            "A root has pushed through the corner. No one has cut it back.",
            "Something small moves in the darkness overhead. Doesn't come closer.",
            "The air smells like earth and old wood.",
        ],
        "mountain" | "highland" => &[
            "The stone here is old — older than the structure built from it.",
            "A narrow window cut into the rock. The view is just more rock.",
            "Cold air comes from somewhere below. There's a gap in the floor you didn't notice.",
            "The ceiling is higher than it needs to be. The builders were thinking of something else.",
            "Iron fixtures, rusting. The bolts hold.",
        ],
        "desert" => &[
            "Sand has drifted in through a gap. No one has swept it.",
            "The heat stays in the walls long after sundown.",
            "A clay jar, empty, standing upright in the corner. Someone left it deliberately.",
            "The dust here is old — the fine kind that settles over years.",
            "No windows. The builders knew better.",
        ],
        "coast" => &[
            "Salt stains on the lower walls. The water came up here once.",
            "The air carries the sea even this far in.",
            "A rope, cut rather than untied. The end frays.",
            "Fish bones in the corner. Someone ate here, not recently.",
        ],
        "plains" | "tundra" => &[
            "The floor is worn smooth by traffic that no longer comes.",
            "A hook on the wall with nothing hanging from it.",
            "The door frame is sized for something taller than a person.",
            "An empty crate, good wood, not worth taking.",
        ],
        _ => &[
            "A candle burned down to nothing on a shelf.",
            "The mortar between the stones is crumbling in one corner.",
            "Footprints in the dust going both ways. Not yours.",
            "Something written on the wall in a language you don't fully read.",
            "A broken lock on the floor. The door it came from is open.",
        ],
    };

    if s % 4 == 0 { // Only fire 25% of cells — not every step narrates
        Some(biome_lines[s % biome_lines.len()])
    } else {
        None
    }
}

/// Returns a scroll's text content — a fragment drawn from the world.
/// Takes the codex event text and wraps it as found writing.
pub fn scroll_text(codex_entry: Option<&str>, province: &str, day: i32) -> String {
    if let Some(entry) = codex_entry {
        // Trim to a readable fragment
        let fragment = entry.split(". ").take(2).collect::<Vec<_>>().join(". ");
        if fragment.len() > 20 && fragment.len() < 200 {
            return format!("A torn page: \"...{fragment}...\"");
        }
    }

    // Fallback fragments — still themed
    let seed = province.len() ^ (day as usize & 0x1F);
    let fragments: &[&str] = &[
        "A partial census. The numbers stop mid-column.",
        "A letter that was never sent. The recipient's name is smeared.",
        "A list of names with dates next to them. Some dates are in the future.",
        "A map of somewhere else, annotated in a hand you don't recognize.",
        "Orders, rescinded. The original order is not included.",
        "A prayer written in the margins of something else.",
        "Supply tallies from six months ago. The numbers don't add up.",
        "A child's drawing of a house. The house is on fire.",
    ];
    fragments[seed % fragments.len()].to_string()
}
