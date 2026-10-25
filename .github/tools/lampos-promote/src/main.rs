use lampos_promote::{Context, github::Github, promote};
use std::{collections::BTreeMap, process::ExitCode};
fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.len() != 2 || arguments[0] != "--sequence" {
        eprintln!("usage: lampos-promote --sequence POSITIVE_INTEGER");
        return ExitCode::from(2);
    }
    let result = (|| {
        let env: BTreeMap<String, String> = std::env::vars().collect();
        let context = Context::environment(&arguments[1], &env)?;
        let receipt = promote(&mut Github, &context)?;
        serde_json::to_string(&receipt).map_err(|_| "cannot encode promotion receipt".to_owned())
    })();
    match result {
        Ok(receipt) => {
            println!("{receipt}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("stable promotion refused: {error}");
            ExitCode::FAILURE
        }
    }
}
