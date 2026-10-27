//! #3113 (the third reviewer's probe on #3127): with `LAMP_MEMORY_FORGET_ALL`'s
//! sources, a kept thing remembered in a conversation carries that
//! conversation's exchanges, and an observation the tidy pass took from the
//! same exchange cites it as evidence. Forgetting one of them takes the
//! shared exchange, never the other, in either direction; what is named and
//! what follows from it are all gone, and "forget everything" still leaves
//! nothing of the person's.
use lamp_memory::{
    Options, Person,
    days::Days,
    tidy::{Category, Output, Proposal},
};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
};

const NOW: u64 = 1_791_892_800_000;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("lamp-forget-shared-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }

    fn open(&self) -> Days {
        Days::open(
            Options {
                dir: self.0.clone(),
                keep_days: 30,
                local: lamp_memory::store::LocalTime::Offset(0),
            },
            NOW,
        )
        .unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Twelve's conversation: the colour said, the wedding remembered with the
/// conversation's exchange as `remember_from` gives it under
/// `LAMP_MEMORY_FORGET_ALL`, then the colour noticed from that exchange.
/// The ids of the exchange, the wedding and the colour.
fn conversation(store: &mut Days, mai: &Person) -> (u64, u64, u64) {
    let said = store
        .append(
            mai,
            NOW,
            "My favourite colour is violet, and my sister's wedding is on Saturday",
            "Lovely",
            &[],
        )
        .unwrap();
    let wedding = store
        .remember_from(mai, NOW + 1, "Sister's wedding is on Saturday.", &[said])
        .unwrap();
    let job = store.conversation_end(mai, "c", &[said]).unwrap();
    let colour = store
        .tidy_apply(
            mai,
            &job.job,
            &Output {
                observations: vec![Proposal {
                    text: "Favourite colour is violet.".into(),
                    category: Category::Preference,
                    source_ids: vec![said],
                    supersedes: None,
                    replaces: None,
                    on: None,
                }],
            },
        )
        .unwrap()[0];
    assert_eq!(
        store.record(mai, wedding).unwrap().unwrap().source_ids,
        [said]
    );
    assert_eq!(
        store.record(mai, colour).unwrap().unwrap().source_ids,
        [said]
    );
    (said, wedding, colour)
}

fn kept_texts(store: &Days, mai: &Person) -> Vec<String> {
    store
        .kept_with(mai, 16_384, true)
        .unwrap()
        .into_iter()
        .map(|kept| kept.text)
        .collect()
}

#[test]
fn forgetting_a_noticed_thing_leaves_a_kept_thing_from_the_same_exchange() {
    let scratch = Scratch::new("colour");
    let mut store = scratch.open();
    let mai = Person::new("p-0123456789abcdef").unwrap();
    let (said, wedding, colour) = conversation(&mut store, &mai);
    // A later exchange that used the colour: it follows from it.
    let echo = store
        .append(&mai, NOW + 2, "What's my colour?", "Violet", &[colour])
        .unwrap();
    assert_eq!(
        kept_texts(&store, &mai),
        [
            "Sister's wedding is on Saturday.",
            "Favourite colour is violet."
        ]
    );
    assert_eq!(store.forget(&mai, colour).unwrap(), 3);
    for id in [colour, said, echo] {
        assert!(store.record(&mai, id).unwrap().is_none(), "{id} is gone");
    }
    let kept = store
        .record(&mai, wedding)
        .unwrap()
        .expect("the wedding stays");
    assert!(kept.source_ids.is_empty(), "{:?}", kept.source_ids);
    assert_eq!(
        kept_texts(&store, &mai),
        ["Sister's wedding is on Saturday."]
    );
    // Reopened, the same.
    drop(store);
    let store = scratch.open();
    assert_eq!(
        kept_texts(&store, &mai),
        ["Sister's wedding is on Saturday."]
    );
}

#[test]
fn forgetting_a_kept_thing_leaves_a_noticed_thing_from_the_same_exchange() {
    let scratch = Scratch::new("wedding");
    let mut store = scratch.open();
    let mai = Person::new("p-0123456789abcdef").unwrap();
    let (said, wedding, colour) = conversation(&mut store, &mai);
    assert_eq!(store.forget(&mai, wedding).unwrap(), 2);
    for id in [wedding, said] {
        assert!(store.record(&mai, id).unwrap().is_none(), "{id} is gone");
    }
    assert_eq!(kept_texts(&store, &mai), ["Favourite colour is violet."]);
    assert!(store.record(&mai, colour).unwrap().is_some());
}

#[test]
fn forgetting_everything_still_leaves_nothing_of_the_persons() {
    let scratch = Scratch::new("everything");
    let mut store = scratch.open();
    let mai = Person::new("p-0123456789abcdef").unwrap();
    let an = Person::new("p-fedcba9876543210").unwrap();
    let (said, wedding, colour) = conversation(&mut store, &mai);
    let theirs = store.remember(&an, NOW, "Tea, no sugar.").unwrap();
    assert_eq!(store.forget_all(Some(&mai)).unwrap(), 3);
    for id in [said, wedding, colour] {
        assert!(store.record(&mai, id).unwrap().is_none(), "{id} is gone");
    }
    assert!(kept_texts(&store, &mai).is_empty());
    assert!(store.record(&an, theirs).unwrap().is_some());
}

/// Eighth's P3 on #3127: a forget that starts from the exchange itself (by
/// its id, as `forget_last` with `kind: exchange` sends it) takes the
/// observation noticed from it, but the kept thing remembered from it stays,
/// as `descendants`' forget walk says: it goes only when it is named.
#[test]
fn forgetting_an_exchange_keeps_its_kept_thing_and_loses_its_observation() {
    let scratch = Scratch::new("exchange");
    let mut store = scratch.open();
    let mai = Person::new("p-0123456789abcdef").unwrap();
    let (said, wedding, colour) = conversation(&mut store, &mai);
    assert_eq!(store.forget(&mai, said).unwrap(), 2);
    for id in [said, colour] {
        assert!(store.record(&mai, id).unwrap().is_none(), "{id} is gone");
    }
    let kept = store
        .record(&mai, wedding)
        .unwrap()
        .expect("the wedding stays");
    assert!(kept.source_ids.is_empty(), "{:?}", kept.source_ids);
    assert_eq!(
        kept_texts(&store, &mai),
        ["Sister's wedding is on Saturday."]
    );
    // Reopened, the same.
    drop(store);
    let store = scratch.open();
    assert_eq!(
        kept_texts(&store, &mai),
        ["Sister's wedding is on Saturday."]
    );
}
