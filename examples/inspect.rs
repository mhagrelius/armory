//! Read a real collector dump and say what is in it.
//!
//! The addon's calls are documented but the returns are not contractual, and
//! several of them — `GetAchievementInfo`'s `earnedBy`, the journals' extra-info
//! shapes, `Enum.BagIndex` for Warband tabs — are the sort of thing that
//! silently returns nil against a live client. A parser that quietly produces an
//! empty map looks identical to an account with nothing in it, so this prints
//! the counts and lets a person see which it is.
//!
//! ```sh
//! cargo run --example inspect
//! cargo run --example inspect -- "/path/to/World of Warcraft/_retail_"
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use armory::model::addon::{self, collector};
use armory::model::plan::{self, Inputs};
use armory::model::run::{Bucket, Run, Standing};
use armory::model::settings;
use armory::model::source::blizzard::collections::{Kind, Source};

fn main() {
    let wow = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| settings::find_wow(&PathBuf::from(std::env::var("HOME").unwrap_or_default())));

    let Some(wow) = wow else {
        eprintln!("no WoW install found — pass the path to _retail_");
        std::process::exit(1);
    };
    println!("install: {}", wow.display());

    let Some(account) = addon::accounts(&wow).into_iter().next() else {
        eprintln!("no account folder under WTF/Account");
        std::process::exit(1);
    };
    println!("account: {account}\n");

    // -- the account-wide file ----------------------------------------------

    let path = addon::account_saved_variables(&wow, &account, "Armory_Collector");
    let source = std::fs::read(&path).expect("the account file");
    let collected = match collector::read(&String::from_utf8_lossy(&source)) {
        Ok(collected) => collected,
        Err(error) => {
            eprintln!("could not read {}: {error}", path.display());
            std::process::exit(1);
        }
    };

    println!("ACCOUNT");
    println!("  written at        {:?}", collected.written_at);
    println!("  attributions      {}", collected.earned_by.len());
    println!("  completed         {}", collected.completed.len());
    println!("  criteria trees    {}", collected.tree.len());
    println!("  criteria mapped   {}", collected.criteria.len());

    let understood = collected
        .criteria
        .values()
        .filter(|kind| kind.is_observable())
        .count();
    println!(
        "  of those, understood {understood} ({:.0}%)",
        percent(understood, collected.criteria.len())
    );

    println!("  warband bank      {} items", collected.warband_bank.len());
    println!(
        "  currencies        {} characters",
        collected.currencies.len()
    );

    for kind in Kind::ALL {
        let all: Vec<_> = collected
            .collectibles
            .iter()
            .filter(|entry| entry.kind == kind)
            .collect();
        let owned = all
            .iter()
            .filter(|entry| collected.owned.contains(&(kind, entry.id)))
            .count();
        let sourced = all
            .iter()
            .filter(|entry| entry.source != Source::Unknown)
            .count();
        println!(
            "  {:<16}  {} known, {owned} owned, {sourced} with a source",
            kind.label(),
            all.len()
        );
    }

    // The thing most worth eyeballing: is the source text actually arriving?
    if let Some(example) = collected
        .collectibles
        .iter()
        .find(|entry| entry.kind == Kind::Mount && entry.source == Source::Drop)
    {
        println!(
            "  example mount     {} — {}",
            example.name,
            example
                .description
                .as_deref()
                .unwrap_or("(no text)")
                .replace('\n', " · ")
        );
    }

    // -- the per-character files --------------------------------------------

    let files = addon::character_files(&wow, &account, "Armory_Collector");
    println!("\nCHARACTERS ({} file(s))", files.len());

    let mut characters = Vec::new();
    for file in &files {
        let source = std::fs::read(file).expect("a character file");
        match collector::read_character(&String::from_utf8_lossy(&source)) {
            Ok(read) => {
                println!(
                    "  {:<24} level {:<3} ilvl {:<5} {} quests, {} professions",
                    read.character.full_name(),
                    read.character.level,
                    read.detail
                        .item_level
                        .map(|l| l.to_string())
                        .unwrap_or_else(|| "?".into()),
                    read.quests.len(),
                    read.detail.professions.len(),
                );
                characters.push(read);
            }
            Err(error) => println!("  {} — {error}", file.display()),
        }
    }

    if characters.is_empty() {
        println!("\nNo character files yet. Log in on someone and log out.");
        return;
    }

    // -- what a run would look like ------------------------------------------

    let cohort = armory::model::cohort::Cohort::from(
        characters
            .iter()
            .map(|read| read.character.key.clone())
            .collect::<Vec<_>>(),
    );

    let mut inputs = Inputs {
        progress: collected.progress(),
        attributions: collected.earned_by.clone(),
        criteria: collected.criteria.clone(),
        owned: collected
            .owned
            .iter()
            .map(|(_, id)| *id)
            .collect::<HashSet<u32>>(),
        ..Inputs::default()
    };
    for read in &characters {
        inputs
            .primary
            .insert(read.character.key.clone(), read.primary());
    }

    let baseline = plan::take_baseline(&inputs.progress, &HashSet::new(), chrono::Utc::now());
    let goals = plan::plan(&baseline, &cohort, &inputs);
    let run = Run {
        name: "Inspection".into(),
        baseline,
        cohort,
        goals,
    };

    let progress = run.progress();
    println!("\nA RUN ENROLLING EVERY CHARACTER SEEN");
    println!("  goals            {}", run.goals.len());
    println!("  settled          {}", progress.done);
    println!("  poisoned         {}", run.poisoned().count());
    println!("    observable     {}", bucket(&run, Bucket::Observable));
    println!("    attestable     {}", bucket(&run, Bucket::Attestable));
    println!("  excluded         {}", progress.excluded);

    let unattributed = run
        .goals
        .iter()
        .filter(|goal| matches!(goal.standing, Standing::Poisoned { by: None }))
        .count();
    println!("  unattributed     {unattributed}");

    // Why so few observable? Because one unmeasurable leaf makes a whole tree
    // unmeasurable, and most criteria types have no per-character source at all.
    let mut kinds: Vec<(String, usize)> = collected
        .criteria
        .values()
        .fold(
            std::collections::HashMap::<String, usize>::new(),
            |mut counts, kind| {
                // The variant, not its payload: "Quest", not "Quest(5000)".
                let name = format!("{kind:?}");
                let name = name.split('(').next().unwrap_or(&name).to_string();
                *counts.entry(name).or_default() += 1;
                counts
            },
        )
        .into_iter()
        .collect();
    kinds.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

    println!("\nWHAT THE CRITERIA MEASURE");
    for (kind, count) in kinds.iter().take(6) {
        println!("  {kind:<20} {count}");
    }
    println!();
    for line in [
        "Only quest- and achievement-backed criteria can be measured against one",
        "character's own data. Everything else — creature kills, exploration, spell",
        "casts — WoW records account-wide only, so a goal containing one of those",
        "goes to attestation rather than to a progress bar.",
    ] {
        println!("  {line}");
    }

    // The payoff, if there is one: poisoned goals with real movement on them.
    let mut moving: Vec<_> = run
        .goals
        .iter()
        .filter(|goal| goal.standing.is_poisoned() && goal.bucket == Bucket::Observable)
        .filter_map(|goal| {
            let evaluation = goal.evaluation.as_ref()?;
            (evaluation.observable && evaluation.progress > 0).then_some((
                goal,
                evaluation.required.saturating_sub(evaluation.progress),
            ))
        })
        .collect();
    moving.sort_by_key(|(goal, remaining)| (*remaining, goal.achievement_id));

    println!("\nCLOSEST TO DONE ({} with progress)", moving.len());
    for (goal, remaining) in moving.iter().take(10) {
        let evaluation = goal.evaluation.as_ref().expect("checked above");
        println!(
            "  achievement {:<7} {}/{} — {remaining} to go",
            goal.achievement_id, evaluation.progress, evaluation.required
        );
    }
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    part as f64 / whole as f64 * 100.0
}

fn bucket(run: &Run, want: Bucket) -> usize {
    run.goals
        .iter()
        .filter(|goal| goal.standing.is_poisoned() && goal.bucket == want)
        .count()
}
