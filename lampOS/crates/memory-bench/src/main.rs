use lamp_memory_bench::{
    Result, corpus, files, invalid, ledger,
    score::{Answer, Question},
};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};
fn run(args: &[String]) -> Result<()> {
    match args {
        [
            command,
            fixture,
            output,
            arm,
            revision,
            registration,
            recorded,
            adapter,
            adapter_revision,
        ] if command == "plumbing-external" => println!(
            "{}",
            lamp_memory_bench::plumbing::run_with_adapter(
                Path::new(fixture),
                Path::new(output),
                arm,
                revision,
                registration,
                Path::new(recorded),
                lamp_memory_bench::plumbing::AdapterSpec {
                    executable: Path::new(adapter),
                    args: vec![],
                    revision: adapter_revision
                }
            )?
        ),
        [command, events, destination, voice, rate] if command == "speech-prepare" => println!(
            "{}",
            lamp_memory_bench::speech::prepare(
                Path::new(events),
                Path::new(destination),
                voice,
                rate.parse().map_err(|_| invalid("invalid TTS rate"))?
            )?
        ),
        [
            command,
            fixture,
            output,
            arm,
            revision,
            registration,
            recorded,
        ] if command == "plumbing" => println!(
            "{}",
            lamp_memory_bench::plumbing::run(
                Path::new(fixture),
                Path::new(output),
                arm,
                revision,
                registration,
                Path::new(recorded)
            )?
        ),
        [command, paths @ ..] if command == "plumbing-card" && !paths.is_empty() => {
            println!("{}", lamp_memory_bench::plumbing::scorecard(paths)?)
        }
        [command, revision] if command == "arm" => {
            lamp_memory_bench::arm_server::serve(
                &mut std::io::stdin().lock(),
                &mut std::io::stdout().lock(),
                revision,
            )?;
        }
        [command, adapter, arm, revision, state] if command == "smoke" => {
            println!(
                "{}",
                lamp_memory_bench::smoke::run(Path::new(adapter), arm, revision, Path::new(state))?
            );
        }
        [command, persona, destination] if command == "generate" => {
            let persona = files::json_file(Path::new(persona))?;
            let corpus = corpus::generate(persona)?;
            files::write_corpus(Path::new(destination), &corpus)?;
            println!(
                "{}",
                json!({"events":corpus.events.len(),"questions":corpus.questions.len(),"categories":corpus::counts(&corpus),"status":"generated; not scored; human review pending"})
            );
        }
        [command, directory] if command == "validate" => {
            let corpus = files::read_corpus(Path::new(directory))?;
            println!(
                "{}",
                json!({"valid":true,"events":corpus.events.len(),"questions":corpus.questions.len(),"categories":corpus::counts(&corpus)})
            );
        }
        [command, directory] if command == "verify-ledger" => println!(
            "{}",
            serde_json::to_string(&ledger::verify(Path::new(directory))?)?
        ),
        [command, questions, answers] if command == "score" => {
            let questions: Vec<Question> = files::lines(Path::new(questions))?;
            let answers: Vec<AnswerRow> = files::lines(Path::new(answers))?;
            let mut by_id = BTreeMap::new();
            for row in answers {
                if by_id.insert(row.id, row.answer).is_some() {
                    return Err(invalid("duplicate answer"));
                }
            }
            if by_id.len() != questions.len() {
                return Err(invalid("answer count differs from registered questions"));
            }
            for q in questions {
                let a = by_id
                    .remove(&q.id)
                    .ok_or_else(|| invalid("missing answer"))?;
                println!(
                    "{}",
                    json!({"id":q.id,"category":q.category,"score":q.score(&a)?})
                );
            }
        }
        _ => {
            return Err(invalid(
                "usage: lamp-memory-bench generate PERSONA NEW_DIRECTORY | validate DIRECTORY | verify-ledger DIRECTORY | score QUESTIONS_JSONL ANSWERS_JSONL | smoke ADAPTER ARM FULL_REVISION NEW_STATE_DIRECTORY",
            ));
        }
    }
    Ok(())
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AnswerRow {
    id: String,
    answer: Answer,
}
fn main() {
    if let Err(error) = run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("lamp-memory-bench: {error}");
        std::process::exit(1);
    }
}
