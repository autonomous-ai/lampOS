use lamp_voice::{config::ProviderConfig, process::parse_boot, transport::WorkerChannels};
use std::{io, path::Path};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn main() {
    // A worker's scheduling, on its first thread before any other starts
    // (#1608, `LAMP_WORKER_SCHEDULING=on`): the audio keeps the voice's RR
    // 20, the ring a lower real-time priority, and the rest the ordinary
    // policy, so none takes the CPU from the lamp's ordinary parts. An
    // unlisted role is refused below as before.
    if std::env::args().nth(1).as_deref() == Some("worker")
        && lamp_voice::worker_scheduling::on(
            std::env::var(lamp_voice::worker_scheduling::ENVIRONMENT)
                .ok()
                .as_deref(),
        )
        && let Some(role) = std::env::args()
            .nth(2)
            .and_then(|role| lamp_voice_processes::WorkerRole::parse(&role))
        && let Err(error) = lamp_voice::worker_scheduling::take(role.scheduling())
    {
        eprintln!(
            "lamp-voice worker {}: its scheduling was not set, so it runs as the voice does: {error}",
            role.as_str()
        );
    }
    // The service and its workers: a log line never blocks them or ends
    // them (`lamp_voice::log_relay`), and their last lines are passed on.
    let relayed = matches!(std::env::args().nth(1).as_deref(), Some("serve" | "worker"))
        && lamp_voice::log_relay::install()
            .map_err(|error| eprintln!("lamp-voice: log lines go straight to the journal: {error}"))
            .is_ok();
    // The stall recorders (#1178) finish their lines once every device is
    // closed: after `run`, or while a panic unwinds out of it.
    let stalls_end = lamp_voice::stalls::Finishing(std::time::Duration::from_millis(300));
    let result = run();
    if let Err(error) = &result {
        eprintln!("lamp-voice: {error}");
    }
    drop(stalls_end);
    if relayed {
        lamp_voice::log_relay::finish(std::time::Duration::from_millis(500));
    }
    if result.is_err() {
        std::process::exit(1);
    }
}
/// `stt-check`'s `[ROUNDS] [--held-ms MS]`, in that order: three rounds,
/// and nothing held, unless said. At most 20 rounds and 1.6 s held, the
/// longest a paused reply holds (#828).
fn stt_check_options(rest: &[String]) -> Result<(u32, u32)> {
    let usage = || io::Error::other("usage: stt-check CONFIG WAV [ROUNDS] [--held-ms MS]");
    let (rounds, rest) = match rest {
        [rounds, rest @ ..] if !rounds.starts_with("--") => {
            (rounds.parse().map_err(|_| usage())?, rest)
        }
        _ => (3, rest),
    };
    let held = match rest {
        [] => 0,
        [flag, ms] if flag == "--held-ms" => ms.parse().map_err(|_| usage())?,
        _ => return Err(usage().into()),
    };
    if !(1..=20).contains(&rounds) || held > 1_600 {
        return Err(usage().into());
    }
    Ok((rounds, held))
}

/// `probe-cascade`'s `[RUNS] [--look JPEG | --wire [--kept] [--speech] | --wire --socket | --wire --sizes]`,
/// in that order: five runs, every candidate and no look unless said; at
/// most 50 runs, 20 with `--kept` or `--socket`. With `--wire`, `Some`, holding how it
/// asks.
fn probe_options(
    rest: &[String],
) -> Result<(
    u32,
    Option<std::path::PathBuf>,
    Option<lamp_voice::second_provider::probe::wire::Mode>,
)> {
    use lamp_voice::second_provider::probe::wire::Mode;
    let usage = || {
        io::Error::other(
            "usage: probe-cascade PROVIDER_CONFIG SECOND_CONFIG [RUNS] [--look JPEG | --wire [--kept] [--speech] | --wire --socket | --wire --sizes]",
        )
    };
    let (runs, rest) = match rest {
        [runs, rest @ ..] if !runs.starts_with("--") => (runs.parse().map_err(|_| usage())?, rest),
        _ => (5, rest),
    };
    let (look, wire) = match rest {
        [] => (None, None),
        [flag, jpeg] if flag == "--look" => (Some(std::path::PathBuf::from(jpeg)), None),
        [flag, how @ ..] if flag == "--wire" => {
            let how: Vec<&str> = how.iter().map(String::as_str).collect();
            let (kept, speech, socket, sizes) = match how.as_slice() {
                [] => (false, false, false, false),
                ["--kept"] => (true, false, false, false),
                ["--speech"] => (false, true, false, false),
                ["--kept", "--speech"] => (true, true, false, false),
                ["--socket"] => (false, false, true, false),
                ["--sizes"] => (false, false, false, true),
                _ => return Err(usage().into()),
            };
            (
                None,
                Some(Mode {
                    kept,
                    speech,
                    socket,
                    sizes,
                }),
            )
        }
        _ => return Err(usage().into()),
    };
    if !(1..=50).contains(&runs)
        || (wire.is_some_and(|mode| mode.kept || mode.socket)
            && runs > lamp_voice::second_provider::probe::wire::MOST_KEPT_RUNS)
    {
        return Err(usage().into());
    }
    Ok((runs, look, wire))
}

fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    // The voice runs only as a command on its list, and a worker only in a
    // role on it: the lists the records guard knows too
    // (lamp-voice-processes, #1071).
    if let Some(command) = args.first()
        && lamp_voice_processes::Command::parse(command).is_none()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("command `{command}` is not on the voice's list (lamp-voice-processes)"),
        )
        .into());
    }
    if let [command, role, ..] = args.as_slice()
        && command == lamp_voice_processes::Command::Worker.as_str()
        && lamp_voice_processes::WorkerRole::parse(role).is_none()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("worker role `{role}` is not on the voice's list (lamp-voice-processes)"),
        )
        .into());
    }
    match args.as_slice() {
        // How often a short sound got an answer, from the lamp's own
        // records (#800): one line a short turn, then counts. Reads only;
        // what was heard is printed as kinds, never as words.
        [command,sessions] if command=="short-turns" => {
            let shorts=lamp_voice::short_turns::short_turns(&lamp_voice::short_turns::records_under(Path::new(sessions))?);
            for short in &shorts { println!("{}",short.row()); }
            println!("{}",lamp_voice::short_turns::summary(&shorts));
        },
        // stt-check CONFIG WAV [ROUNDS] [--held-ms MS]: how soon the second
        // voice's speech-to-text (its CONFIG, the file
        // LAMP_VOICE_SECOND_CONFIG names) gives back the first words of a
        // recorded line, 16 kHz mono (#828). The first MS of the line go at
        // once, in one message (the route sends 20 ms frames); the rest at
        // the pace it was said, in 20 ms frames. Its times run from audio
        // queued to the stream to a result parsed from it: not the wire, and
        // not a stop's latency. One line a round, with the words heard.
        [command, config, wav, rest @ ..] if command == "stt-check" && rest.len() <= 3 => {
            let (rounds, held_ms) = stt_check_options(rest)?;
            // The unit's switches too: LAMP_SECOND_FLUX=on hears the line
            // with Flux, LAMP_SECOND_KEY_TERMS adds its words.
            let config = lamp_voice::second_config::load_from_path(Path::new(config))?
                .with_environment(|name| std::env::var_os(name))?;
            let mut reader = hound::WavReader::open(wav)?;
            let spec = reader.spec();
            if spec.sample_rate != 16_000 || spec.channels != 1 || spec.bits_per_sample != 16 {
                return Err(io::Error::other("stt-check: the line must be 16 kHz mono 16-bit").into());
            }
            let samples = reader.samples::<i16>().collect::<std::result::Result<Vec<_>, _>>()?;
            let held = samples.len().min(held_ms as usize * 16);
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
                use tokio::time::{Duration, Instant, interval};
                for round in 1..=rounds {
                    let asked = Instant::now();
                    let stream = lamp_voice::second_provider::net::SttStream::connect(&config).await?;
                    let connected = asked.elapsed();
                    let first_words = stream.first_words();
                    let sent = Instant::now();
                    if held > 0 {
                        stream.send(&samples[..held])?;
                    }
                    let mut pace = interval(Duration::from_millis(20));
                    for frame in samples[held..].chunks(320) {
                        pace.tick().await;
                        stream.send(frame)?;
                    }
                    let audio_end = Instant::now();
                    let words = stream.finish(u64::from(round)).await?;
                    let finished = audio_end.elapsed();
                    let first = first_words
                        .get()
                        .map_or_else(|| "none".to_owned(), |at| at.duration_since(sent).as_millis().to_string());
                    println!(
                        "round={round} held_ms={held_ms} connect_ms={} first_words_ms={first} audio_ms={} final_after_audio_ms={} words={words:?}",
                        connected.as_millis(),
                        audio_end.duration_since(sent).as_millis(),
                        finished.as_millis()
                    );
                }
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
            })?;
        },
        // The stalls `LAMP_STALLS=on` recorded (#1178), longest first with
        // what held each: `lamp-voice stalls [DIR] [--minutes N]`, the
        // voice's run folder and the last 15 minutes unless given. Reads only.
        [command, rest @ ..] if command == "stalls" && rest.len() <= 3 => {
            let mut dir = std::path::PathBuf::from(lamp_voice::stalls::DEFAULT_DIR);
            let mut minutes = 15u64;
            let mut words = rest.iter();
            while let Some(word) = words.next() {
                if word == "--minutes" {
                    minutes = words
                        .next()
                        .and_then(|n| n.parse().ok())
                        .ok_or("--minutes takes a whole number")?;
                } else {
                    dir = word.into();
                }
            }
            print!(
                "{}",
                lamp_voice::stalls::summary(&dir, lamp_ipc::monotonic_us(), minutes * 60_000_000)?
            );
        },
        [command] if command == "schedule-consumer" => {
            lamp_voice::reminders::run_consumer()?;
        },
        // marked [MARKS]: the moments people marked for review, newest first.
        [command, rest @ ..] if command == "marked" && rest.len() <= 1 => {
            let marks = rest.first().map_or(lamp_voice::marked::DEFAULT_MARKS, String::as_str);
            for line in lamp_voice::marked::list(Path::new(marks))? {
                println!("{line}");
            }
        },
        // The same with one JPEG beside the note: the lamp taking a look.
        [command,path,flag,picture,note] if command=="provider-check" && flag=="--look" => {
            let config=ProviderConfig::load(Path::new(path))?.into_session()?;
            let jpeg=std::fs::read(picture)?;
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
                use lamp_gemini::Event;
                let mut connection=lamp_gemini::connect(config,lamp_gemini::SessionId::new(1)?).await?;
                let input=connection.input();
                let request=lamp_gemini::RequestId::new(1)?;
                input.try_written(request,note,Some(&jpeg))?;
                let asked=lamp_ipc::monotonic_us();
                let (mut said,mut samples,mut first,mut ended)=(String::new(),0usize,None,"timeout");
                let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(15);
                loop {
                    match tokio::time::timeout_at(deadline,connection.next_event()).await {
                        Ok(Some(Event::Audio{pcm,..})) => { first.get_or_insert(lamp_ipc::monotonic_us()-asked); samples+=pcm.len(); }
                        Ok(Some(Event::OutputTranscript{text,..})) => said.push_str(&text),
                        Ok(Some(Event::TurnComplete{..})) => { ended="turn_complete"; break; }
                        Ok(Some(Event::Unanswered{..})) => { ended="unanswered"; break; }
                        Ok(Some(_)) => {}
                        Ok(None) => { ended="connection_ended"; break; }
                        Err(_) => break,
                    }
                }
                println!("{}",serde_json::json!({"status":"look_tried","picture_bytes":jpeg.len(),"ended":ended,"first_audio_us":first,"audio_samples":samples,"said":said,"state":format!("{:?}",connection.state()),"microphone_opened":false,"audio_sent":false}));
                connection.shutdown();
                Ok::<(),Box<dyn std::error::Error+Send+Sync>>(())
            })?;
        },
        // A whole turn with a written note in place of speech, to see what the
        // service makes of one. Nothing is captured and nothing is played.
        // Where each of the lamp's turns spent its time, from its own
        // records (#799): the session records under SESSIONS and, if given,
        // the first-output files under FIRST_OUTPUT. Reads only; prints one
        // line a turn, then medians. Numbers and function names, no words.
        [command,sessions,rest @ ..] if command=="turn-times" && rest.len()<=1 => {
            use lamp_voice::turn_times::{lines_under,summary,turns};
            let events=lines_under(Path::new(sessions),"events-")?;
            let first=match rest.first() { Some(directory)=>lines_under(Path::new(directory),"first-output-")?, None=>Vec::new() };
            let turns=turns(&events,&first);
            for turn in &turns { println!("{}",turn.row()); }
            println!("{}",summary(&turns));
        },
        // Where a turn with a function call spends its time (#799): the
        // fixed requests of `tool_turns::SCRIPT`, ROUNDS times, each on a
        // connection of its own, told of the voice's own functions and the
        // agent tasks' as on the lamp. Every call is answered at once by a
        // stand-in: nothing reaches the tasks part or anything else, and
        // nothing is captured or played. One line a request, then medians.
        // `--thinking TOKENS` sets the thinking budget for this run only, to
        // compare with the one in CONFIG.
        [command,path,flag,rounds,rest @ ..] if command=="provider-check" && (flag=="--tool-turns" || flag=="--hand-overs") && matches!(rest.len(),0|2) => {
            use lamp_voice::tool_turns::{HAND_OVERS,PAUSE,SCRIPT,Timeline,failed,next_pause,stand_in,summary_of};
            // --hand-overs: the requests of #854 instead, asked of Claude Code or of the lamp.
            let script=if flag=="--hand-overs" { HAND_OVERS } else { SCRIPT };
            let rounds:usize=rounds.parse().ok().filter(|rounds|(1..=20).contains(rounds)).ok_or("ROUNDS is a number from 1 to 20")?;
            let thinking:Option<u32>=match rest { [flag,tokens] if flag=="--thinking"=>Some(tokens.parse().map_err(|_|"--thinking takes a number of tokens")?), []=>None, _=>return Err("provider-check CONFIG --tool-turns|--hand-overs ROUNDS [--thinking TOKENS]".into()) };
            // LAMP_TOOL_LEAD_IN=on: the functions and their answers as with the lead-in (#799).
            let lead_in=std::env::var(lamp_voice::agent_tasks::LEAD_IN_ENVIRONMENT).as_deref()==Ok("on");
            // LAMP_TASKS_ASKED=on: the functions as with work asked of the staff handed over every time (#854).
            let asked=std::env::var(lamp_voice::agent_tasks::ASKED_ENVIRONMENT).as_deref()==Ok("on");
            // LAMP_TASKS_QUESTIONS=on, with asked: a question for the staff is theirs, as a question task.
            let questions=asked&&std::env::var(lamp_voice::agent_tasks::QUESTIONS_ENVIRONMENT).as_deref()==Ok("on");
            let place=lamp_voice::agent_tasks::Place{directory:"/nonexistent".into(),target:lamp_voice::agent_tasks::DEFAULT_TARGET.into(),who:false,lead_in,asked,accounts:false,machine_names:false,needs:None,staff_note:false,make_skill:false,named_only:false,correct:false,sign_in:false,questions};
            // With the switch, each session is told after the persona what a lamp's would be (#854).
            let note=lamp_voice::agent_tasks::session_note(&place).unwrap_or_default();
            let colour=lamp_gemini::Tool{name:"light_colour".into(),description:"Set the colour of your own light when you are asked to.".into(),parameters:Some(serde_json::json!({"type":"object","properties":{"colour":{"type":"string","enum":["white","warm","red","green","blue","purple"]}},"required":["colour"]})),silent:false};
            let tools:Vec<lamp_gemini::Tool>=lamp_voice::tools::declared(&lamp_voice::tools::Settings{decline:true,exec:None,memory:None,clock:false,..Default::default()}).into_iter().chain([colour]).chain(lamp_voice::agent_tasks::tools(&place)).collect();
            let mut results=Vec::new();
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
                use lamp_gemini::Event;
                // One session after another, never two at once, with a pause
                // between them that grows after each that could not be opened.
                let mut pause=PAUSE;
                let mut first=true;
                for round in 0..rounds {
                    for scripted in script {
                        if !first { tokio::time::sleep(pause).await; }
                        first=false;
                        let mut config=ProviderConfig::load(Path::new(path))?.into_session()?.tools(tools.clone())?;
                        if !note.is_empty() { let written=config.instruction_text().to_owned(); config=config.instruction(&format!("{written}{note}"))?; }
                        if let Some(tokens)=thinking { config=config.thinking_budget(tokens)?; }
                        let mut connection=match lamp_gemini::connect(config,lamp_gemini::SessionId::new(1)?).await {
                            Ok(connection)=>connection,
                            Err(error)=>{
                                let row=failed(scripted,round,"connect",&error.to_string());
                                println!("{row}");
                                results.push(row);
                                pause=next_pause(pause,true);
                                continue;
                            }
                        };
                        pause=next_pause(pause,false);
                        let input=connection.input();
                        let mut timeline=Timeline::default();
                        let mut broke=None;
                        for (index,line) in scripted.lines.iter().enumerate() {
                            // Only the last line is timed; those before it set the scene.
                            let timed=index+1==scripted.lines.len();
                            if let Err(error)=input.try_written(lamp_gemini::RequestId::new(index as u64+1)?,line,None) {
                                broke=Some(error.to_string());
                                break;
                            }
                            let asked=lamp_ipc::monotonic_us();
                            let since=||lamp_ipc::monotonic_us().saturating_sub(asked);
                            let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(14);
                            loop {
                                match tokio::time::timeout_at(deadline,connection.next_event()).await {
                                    Ok(Some(Event::ToolCall{call,..})) => {
                                        if timed { timeline.call(&call.name,since()); }
                                        let answer=stand_in(&call.name,&call.args,lead_in);
                                        if input.try_tool_response(&call.id,&call.name,&answer).is_ok() && timed { timeline.answered(since()); }
                                    }
                                    Ok(Some(Event::Audio{pcm,..})) => if timed { timeline.audio(pcm.len(),since()); },
                                    Ok(Some(Event::OutputTranscript{text,..})) => if timed { timeline.said(&text); },
                                    Ok(Some(Event::TurnComplete{..})) => { if timed { timeline.ended("turn_complete",since()); } break; }
                                    Ok(Some(Event::Unanswered{..})) => { if timed { timeline.ended("unanswered",since()); } break; }
                                    Ok(Some(_)) => {}
                                    Ok(None) => { timeline.ended("connection_ended",since()); break; }
                                    Err(_) => { timeline.ended("timeout",since()); break; }
                                }
                            }
                        }
                        let row=match broke {
                            Some(error)=>failed(scripted,round,"write",&error),
                            None=>timeline.result(scripted,round),
                        };
                        println!("{row}");
                        results.push(row);
                        connection.shutdown();
                    }
                }
                Ok::<(),Box<dyn std::error::Error+Send+Sync>>(())
            })?;
            let mut summary=summary_of(&results,script);
            summary["asked_switch"]=serde_json::json!(asked);
            summary["thinking_budget"]=serde_json::json!(thinking.map_or("as in CONFIG".to_owned(),|tokens|tokens.to_string()));
            summary["lead_in"]=serde_json::json!(lead_in);
            println!("{summary}");
        },
        // A written request to a session that was told of two functions:
        // the voice's own way of saying a thing was not for it, and one of
        // the kind the executive will offer. Does the service take the
        // declarations, which does the model call for which words, what
        // does it say after the answer, and how long does each step take?
        // Every call is answered as done. Nothing is captured or played,
        // and nothing on the lamp is changed.
        [command,path,flag,then,note] if command=="provider-check" && flag=="--tools" && then=="--written" => {
            let own=lamp_voice::tools::declared(&lamp_voice::tools::Settings{decline:true,exec:None,memory:None,clock:false,..Default::default()});
            let colour=lamp_gemini::Tool{name:"light_colour".into(),description:"Set the colour of your own light when you are asked to.".into(),parameters:Some(serde_json::json!({"type":"object","properties":{"colour":{"type":"string","enum":["white","warm","red","green","blue","purple"]}},"required":["colour"]})),silent:false};
            // LAMP_PROBE_PLAIN set: the voice's own function is declared as an ordinary one, to compare.
            let own:Vec<lamp_gemini::Tool>=own.into_iter().map(|mut tool|{tool.silent=std::env::var_os("LAMP_PROBE_PLAIN").is_none();tool}).collect();
            let config=ProviderConfig::load(Path::new(path))?.into_session()?.tools(own.into_iter().chain([colour]).collect())?;
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
                use lamp_gemini::Event;
                let started=lamp_ipc::monotonic_us();
                let mut connection=lamp_gemini::connect(config,lamp_gemini::SessionId::new(1)?).await?;
                let setup_us=lamp_ipc::monotonic_us()-started;
                let input=connection.input();
                input.try_written(lamp_gemini::RequestId::new(1)?,note,None)?;
                let asked=lamp_ipc::monotonic_us();
                let (mut said,mut samples,mut first,mut ended)=(String::new(),0usize,None,"timeout");
                let mut calls=Vec::new();
                // The order of what came, with the time of each, audio once.
                let mut order=Vec::new();
                let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(14);
                loop {
                    let event=tokio::time::timeout_at(deadline,connection.next_event()).await;
                    let at_ms=(lamp_ipc::monotonic_us()-asked)/1000;
                    let kind=match &event {
                        Ok(Some(Event::ToolCall{..}))=>Some("call"),
                        Ok(Some(Event::ToolCallsCancelled{..}))=>Some("calls_withdrawn"),
                        Ok(Some(Event::Audio{..})) if first.is_none()=>Some("first_audio"),
                        Ok(Some(Event::GenerationComplete{..}))=>Some("generation_complete"),
                        Ok(Some(Event::TurnComplete{..}))=>Some("turn_complete"),
                        Ok(Some(Event::Unanswered{..}))=>Some("unanswered"),
                        Ok(Some(Event::Discarded{..}))=>Some("discarded"),
                        Ok(Some(Event::WaitingForInput{..}))=>Some("waiting_for_input"),
                        _=>None,
                    };
                    if let Some(kind)=kind { order.push(format!("{kind}@{at_ms}")); }
                    match event {
                        Ok(Some(Event::ToolCall{call,..})) => {
                            let answer=if call.name==lamp_voice::tools::NOT_FOR_ME { lamp_voice::tools::declined() } else { serde_json::json!({"done":true}) };
                            let sent=input.try_tool_response(&call.id,&call.name,&answer).is_ok();
                            calls.push(serde_json::json!({"name":call.name,"args":call.args,"after_us":lamp_ipc::monotonic_us()-asked,"audio_samples_before":samples,"answer_sent":sent}));
                        }
                        Ok(Some(Event::ToolCallsCancelled{ids,..})) => calls.push(serde_json::json!({"withdrawn":ids.len()})),
                        Ok(Some(Event::Audio{pcm,..})) => { first.get_or_insert(lamp_ipc::monotonic_us()-asked); samples+=pcm.len(); }
                        Ok(Some(Event::OutputTranscript{text,..})) => said.push_str(&text),
                        Ok(Some(Event::TurnComplete{..})) => { ended="turn_complete"; break; }
                        Ok(Some(Event::Unanswered{..})) => { ended="unanswered"; break; }
                        Ok(Some(_)) => {}
                        Ok(None) => { ended="connection_ended"; break; }
                        Err(_) => break,
                    }
                }
                println!("{}",serde_json::json!({"status":"tools_tried","setup_us":setup_us,"ended":ended,"ended_after_us":lamp_ipc::monotonic_us()-asked,"order":order,"calls":calls,"first_audio_us":first,"audio_samples":samples,"said":said,"state":format!("{:?}",connection.state()),"microphone_opened":false,"audio_sent":false}));
                connection.shutdown();
                Ok::<(),Box<dyn std::error::Error+Send+Sync>>(())
            })?;
        },
        [command,path,flag,note] if command=="provider-check" && (flag=="--note" || flag=="--written") => {
            let config=ProviderConfig::load(Path::new(path))?.into_session()?;
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
                use lamp_gemini::Event;
                let started=lamp_ipc::monotonic_us();
                let mut connection=lamp_gemini::connect(config,lamp_gemini::SessionId::new(1)?).await?;
                let setup_us=lamp_ipc::monotonic_us()-started;
                let input=connection.input();
                let request=lamp_gemini::RequestId::new(1)?;
                if flag=="--written" {
                    input.try_written(request,note,None)?;
                } else {
                    input.try_start(request)?;
                    input.try_note(request,note)?;
                    input.try_end(request)?;
                }
                let asked=lamp_ipc::monotonic_us();
                let (mut said,mut samples,mut first,mut ended)=(String::new(),0usize,None,"timeout");
                let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(12);
                loop {
                    match tokio::time::timeout_at(deadline,connection.next_event()).await {
                        Ok(Some(Event::Audio{pcm,..})) => { first.get_or_insert(lamp_ipc::monotonic_us()-asked); samples+=pcm.len(); }
                        Ok(Some(Event::OutputTranscript{text,..})) => said.push_str(&text),
                        Ok(Some(Event::TurnComplete{..})) => { ended="turn_complete"; break; }
                        Ok(Some(Event::Unanswered{..})) => { ended="unanswered"; break; }
                        Ok(Some(_)) => {}
                        Ok(None) => { ended="connection_ended"; break; }
                        Err(_) => break,
                    }
                }
                println!("{}",serde_json::json!({"status":"note_tried","form":flag,"setup_us":setup_us,"ended":ended,"first_audio_us":first,"audio_samples":samples,"said":said,"state":format!("{:?}",connection.state()),"microphone_opened":false,"audio_sent":false}));
                connection.shutdown();
                Ok::<(),Box<dyn std::error::Error+Send+Sync>>(())
            })?;
        },
        // A note for the model beside a request's sound, as the worker gives
        // one when the lamp's name is heard: a recorded question is sent as if
        // heard (16 kHz, one channel, 16-bit little-endian, no header), the
        // note is offered half a second in, and the request ends. Whether the
        // link takes a note in that place, and what is answered. Nothing is
        // captured or played.
        [command,path,flag,note,sound] if command=="provider-check" && flag=="--note-in-speech" => {
            let config=ProviderConfig::load(Path::new(path))?.into_session()?;
            let samples:Vec<i16>=std::fs::read(sound)?.chunks_exact(2).map(|pair| i16::from_le_bytes([pair[0],pair[1]])).collect();
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
                use lamp_gemini::Event;
                let mut connection=lamp_gemini::connect(config,lamp_gemini::SessionId::new(1)?).await?;
                let input=connection.input();
                let request=lamp_gemini::RequestId::new(1)?;
                input.try_start(request)?;
                let mut noted=false;
                for (sequence,block) in samples.chunks(lamp_gemini::MAX_INPUT_SAMPLES).enumerate() {
                    input.try_audio(request,sequence as u64,std::time::Instant::now(),block)?;
                    if !noted && sequence>=25 {
                        input.try_note(request,note)?;
                        noted=true;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                if !noted {
                    input.try_note(request,note)?;
                }
                input.try_end(request)?;
                let asked=lamp_ipc::monotonic_us();
                let (mut heard,mut said,mut audio,mut first,mut ended)=(String::new(),String::new(),0usize,None,"timeout");
                let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(12);
                loop {
                    match tokio::time::timeout_at(deadline,connection.next_event()).await {
                        Ok(Some(Event::Audio{pcm,..})) => { first.get_or_insert(lamp_ipc::monotonic_us()-asked); audio+=pcm.len(); }
                        Ok(Some(Event::OutputTranscript{text,..})) => said.push_str(&text),
                        Ok(Some(Event::UncorrelatedInputTranscript{text,..})) => heard.push_str(&text),
                        Ok(Some(Event::TurnComplete{..})) => { ended="turn_complete"; break; }
                        Ok(Some(Event::Unanswered{..})) => { ended="unanswered"; break; }
                        Ok(Some(_)) => {}
                        Ok(None) => { ended="connection_ended"; break; }
                        Err(_) => break,
                    }
                }
                println!("{}",serde_json::json!({"status":"note_in_speech_tried","sound_ms":samples.len()/16,"ended":ended,"heard":heard,"first_audio_us":first,"audio_samples":audio,"said":said,"state":format!("{:?}",connection.state()),"microphone_opened":false,"audio_played":false}));
                connection.shutdown();
                Ok::<(),Box<dyn std::error::Error+Send+Sync>>(())
            })?;
        },
        // Something told to the service that it should not answer, then a
        // second of near-silence sent as if heard, then a written request: does
        // a session take sound after such a fact, and still answer afterwards?
        // Nothing is captured or played.
        [command,path,flag,context,then,note] if command=="provider-check" && flag=="--context" && then=="--then-sound-and-written" => {
            let config=ProviderConfig::load(Path::new(path))?.into_session()?;
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
                use lamp_gemini::Event;
                let mut connection=lamp_gemini::connect(config,lamp_gemini::SessionId::new(1)?).await?;
                let input=connection.input();
                input.try_context(context)?;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let heard=lamp_gemini::RequestId::new(1)?;
                input.try_start(heard)?;
                let mut noise=0x2545_f491u32;
                for sequence in 0..50u64 {
                    let block:Vec<i16>=(0..lamp_gemini::MAX_INPUT_SAMPLES).map(|_| { noise=noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); ((noise>>24) as i16)-128 }).collect();
                    input.try_audio(heard,sequence,std::time::Instant::now(),&block)?;
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                input.try_end(heard)?;
                let (mut sound_ended,mut sound_said,mut sound_samples)=("timeout",String::new(),0usize);
                let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(8);
                loop {
                    match tokio::time::timeout_at(deadline,connection.next_event()).await {
                        Ok(Some(Event::Audio{pcm,..})) => sound_samples+=pcm.len(),
                        Ok(Some(Event::OutputTranscript{text,..})) => sound_said.push_str(&text),
                        Ok(Some(Event::TurnComplete{..})) => { sound_ended="turn_complete"; break; }
                        Ok(Some(Event::Unanswered{..})) => { sound_ended="unanswered"; break; }
                        Ok(Some(_)) => {}
                        Ok(None) => { sound_ended="connection_ended"; break; }
                        Err(_) => break,
                    }
                }
                let state_after_sound=format!("{:?}",connection.state());
                let (mut said,mut samples,mut ended)=(String::new(),0usize,"not_tried");
                if sound_ended!="connection_ended" {
                    ended="timeout";
                    input.try_written(lamp_gemini::RequestId::new(2)?,note,None)?;
                    let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(12);
                    loop {
                        match tokio::time::timeout_at(deadline,connection.next_event()).await {
                            Ok(Some(Event::Audio{pcm,..})) => samples+=pcm.len(),
                            Ok(Some(Event::OutputTranscript{text,..})) => said.push_str(&text),
                            Ok(Some(Event::TurnComplete{..})) => { ended="turn_complete"; break; }
                            Ok(Some(Event::Unanswered{..})) => { ended="unanswered"; break; }
                            Ok(Some(_)) => {}
                            Ok(None) => { ended="connection_ended"; break; }
                            Err(_) => break,
                        }
                    }
                }
                println!("{}",serde_json::json!({"status":"context_then_sound_tried","sound_ended":sound_ended,"sound_said":sound_said,"sound_audio_samples":sound_samples,"state_after_sound":state_after_sound,"written_ended":ended,"written_said":said,"written_audio_samples":samples,"state":format!("{:?}",connection.state()),"microphone_opened":false,"audio_played":false}));
                connection.shutdown();
                Ok::<(),Box<dyn std::error::Error+Send+Sync>>(())
            })?;
        },
        // Something told to the service that it should not answer, and then a
        // written request that it should: whether the first is taken in, and
        // whether anything comes back for it. Nothing is captured or played.
        [command,path,flag,context,then,note] if command=="provider-check" && flag=="--context" && then=="--written" => {
            let config=ProviderConfig::load(Path::new(path))?.into_session()?;
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
                use lamp_gemini::Event;
                let mut connection=lamp_gemini::connect(config,lamp_gemini::SessionId::new(1)?).await?;
                let input=connection.input();
                input.try_context(context)?;
                // Three seconds for anything to come back for the context alone.
                let (mut back_samples,mut back_said)=(0usize,String::new());
                let quiet_until=tokio::time::Instant::now()+std::time::Duration::from_secs(3);
                while let Ok(Some(event))=tokio::time::timeout_at(quiet_until,connection.next_event()).await {
                    match event {
                        Event::Audio{pcm,..} => back_samples+=pcm.len(),
                        Event::OutputTranscript{text,..} => back_said.push_str(&text),
                        _ => {}
                    }
                }
                input.try_written(lamp_gemini::RequestId::new(1)?,note,None)?;
                let asked=lamp_ipc::monotonic_us();
                let (mut said,mut samples,mut first,mut ended)=(String::new(),0usize,None,"timeout");
                let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(12);
                loop {
                    match tokio::time::timeout_at(deadline,connection.next_event()).await {
                        Ok(Some(Event::Audio{pcm,..})) => { first.get_or_insert(lamp_ipc::monotonic_us()-asked); samples+=pcm.len(); }
                        Ok(Some(Event::OutputTranscript{text,..})) => said.push_str(&text),
                        Ok(Some(Event::TurnComplete{..})) => { ended="turn_complete"; break; }
                        Ok(Some(Event::Unanswered{..})) => { ended="unanswered"; break; }
                        Ok(Some(_)) => {}
                        Ok(None) => { ended="connection_ended"; break; }
                        Err(_) => break,
                    }
                }
                println!("{}",serde_json::json!({"status":"context_tried","after_context_audio_samples":back_samples,"after_context_said":back_said,"ended":ended,"first_audio_us":first,"audio_samples":samples,"said":said,"state":format!("{:?}",connection.state()),"microphone_opened":false,"audio_played":false}));
                connection.shutdown();
                Ok::<(),Box<dyn std::error::Error+Send+Sync>>(())
            })?;
        },
        [command, config, wav] if command == "stt-check" => {
            lamp_voice::stop_words::stt_check(Path::new(config), Path::new(wav))?;
        }
        [command,path] if command=="provider-check" => {
            let config=ProviderConfig::load(Path::new(path))?.into_session()?;
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
                let started=lamp_ipc::monotonic_us();
                let connection=lamp_gemini::connect(config,lamp_gemini::SessionId::new(1)?).await?;
                println!("{}",serde_json::json!({"status":"setup_complete","setup_us":lamp_ipc::monotonic_us()-started,"microphone_opened":false,"audio_sent":false}));
                connection.shutdown();
                Ok::<(),Box<dyn std::error::Error+Send+Sync>>(())
            })?;
        },
        #[cfg(target_os = "linux")]
        [command, config, seconds, output] if command == "camera-inspect" => {
            lamp_voice::camera_inspect::run(Path::new(config), seconds.parse()?, Path::new(output))?;
        }
        #[cfg(target_os = "linux")]
        [command, role, dir, parent, worker, config, digest, flags @ ..] if command == "worker" && role == "camera" => {
            let parent = parse_boot(parent)?;
            let worker = parse_boot(worker)?;
            let mut channels = WorkerChannels::bind(Path::new(dir), role, parent, worker, true)?;
            channels.connect(Path::new(dir), role, true)?;
            let consumers = lamp_voice::camera_worker::parse_frame_consumers(flags)?;
            lamp_voice::camera_worker::run(channels, parent, worker, Path::new(config), digest, Path::new(dir), &consumers)?;
        }
        #[cfg(target_os = "linux")]
        [command, wav, sha256, seconds, output, flags @ ..] if command == "directed-fixture" => {
            let options = lamp_voice::fixture_provider::FixtureOptions::parse(flags)?;
            let seconds = seconds.parse()?;
            options.check_seconds(seconds)?;
            lamp_voice::coordinator::run_fixture(Path::new(wav), sha256, seconds, Path::new(output), options)?;
        }
        [command, role, dir, parent, worker, flag, wav, sha256, timed @ ..]
            if command == "worker" && role == "provider" && flag == "--fixture"
                && (timed.is_empty() || timed == [lamp_voice::fixture_provider::TIMED_FLAG]) => {
            let parent = parse_boot(parent)?;
            let worker = parse_boot(worker)?;
            let mut channels = WorkerChannels::bind(Path::new(dir), role, parent, worker, true)?;
            channels.connect(Path::new(dir), role, true)?;
            lamp_voice::fixture_provider::run(channels, parent, Path::new(wav), sha256, !timed.is_empty())?;
        }
        #[cfg(target_os = "linux")]
        [command, config, output, flags @ ..] if command == "serve" => {
            let options = lamp_voice::options::SessionOptions::parse(flags)?;
            options.validate_serve()?;
            lamp_voice::coordinator::run_serve(Path::new(config), Path::new(output), options)?;
        }
        #[cfg(target_os = "linux")]
        [command, config, seconds, output, flags @ ..] if command == "directed" => {
            let options = lamp_voice::options::SessionOptions::parse(flags)?;
            lamp_voice::coordinator::run_directed_session(Path::new(config), seconds.parse()?, Path::new(output), options)?;
        }
        #[cfg(target_os = "linux")]
        [command, role, dir, parent, worker, device, flags @ ..]
            if command == "worker" && matches!(role.as_str(), "capture" | "speaker") => {
            let options = lamp_voice::options::AudioWorkerOptions::parse(flags, role == "capture")?;
            let parent = parse_boot(parent)?;
            let worker = parse_boot(worker)?;
            let mut channels = WorkerChannels::bind(Path::new(dir), role, parent, worker, true)?;
            channels.connect(Path::new(dir), role, true)?;
            if role == "capture" {
                lamp_voice::audio_workers::capture_with_options(channels,parent,worker,Path::new(dir),device,&options)?;
            } else {
                lamp_voice::audio_workers::speaker(channels,parent,worker,Path::new(dir),device,options.diagnostic_directory.as_deref())?;
            }
        }
        [command, role, dir, parent, worker, extra, flags @ ..] if command == "worker" && role == "provider" => {
            let parent = parse_boot(parent)?;
            let worker = parse_boot(worker)?;
            let mut channels = WorkerChannels::bind(Path::new(dir), role, parent, worker, true)?;
            channels.connect(Path::new(dir), role, true)?;
            match role.as_str() {
                "provider" => {
                    let (continuation, sight, files, visual) = lamp_voice::provider_worker::parse_flags(Path::new(dir), flags)?;
                    let visual = visual.map(|source| (source, worker));
                    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(lamp_voice::provider_worker::run_with_continuation(channels,parent,Path::new(extra),lamp_voice::provider_worker::Reporting::Coordinated,visual,continuation,sight,files))?;
                },
                _ => return Err(io::Error::other("unknown worker role").into()),
            }
        }
        // worker second DIR PARENT WORKER PROVIDER_CONFIG SECOND_CONFIG [--wait-cue] [--down-line] [--only-voice] [--gemini-chat] [--gemini-speech] [the provider worker's flags but --keep-link]
        [command, role, dir, parent, worker, provider_config, second_config, flags @ ..] if command == "worker" && role == "second" => {
            let (route, flags) = lamp_voice::second_provider::worker::RouteFlags::take(flags);
            let (_, _, files, _) = lamp_voice::second_provider::worker::parse_flags(Path::new(dir), flags)?;
            let parent = parse_boot(parent)?;
            let worker = parse_boot(worker)?;
            let mut channels = WorkerChannels::bind(Path::new(dir), role, parent, worker, true)?;
            // The only voice takes its input late, as the Live worker does.
            if route.only_voice {
                channels = channels.taking_late_input();
            }
            channels.connect(Path::new(dir), role, true)?;
            // Both boots, named: the coordinator's for the authority it sends,
            // the worker's own for the offers it stamps (#2793, #3319).
            let boots = lamp_voice::second_provider::worker::WorkerBoots { coordinator: parent, own: worker };
            lamp_voice::second_provider::worker::serve(channels, boots, Path::new(provider_config), Path::new(second_config), files, route)?;
        }
        // probe-cache PROVIDER_CONFIG SECOND_CONFIG [RUNS]
        [command, provider_config, second_config, rest @ ..] if command == "probe-cache" => {
            let usage = || io::Error::other("usage: probe-cache PROVIDER_CONFIG SECOND_CONFIG [RUNS of 1 to 20]");
            let runs = match rest {
                [] => 3,
                [runs] => runs.parse().ok().filter(|runs| (1..=20).contains(runs)).ok_or_else(usage)?,
                _ => return Err(usage().into()),
            };
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(
                lamp_voice::second_provider::probe::cache::run(Path::new(provider_config), Path::new(second_config), runs),
            )?;
        }
        // probe-cut-in PROVIDER_CONFIG SECOND_CONFIG [RUNS]
        [command, provider_config, second_config, rest @ ..] if command == "probe-cut-in" => {
            let usage = || io::Error::other("usage: probe-cut-in PROVIDER_CONFIG SECOND_CONFIG [RUNS of 1 to 20]");
            let runs = match rest {
                [] => 3,
                [runs] => runs.parse().ok().filter(|runs| (1..=20).contains(runs)).ok_or_else(usage)?,
                _ => return Err(usage().into()),
            };
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(
                lamp_voice::second_provider::probe::cut_in::run(Path::new(provider_config), Path::new(second_config), runs),
            )?;
        }
        // probe-tools PROVIDER_CONFIG SECOND_CONFIG [RUNS] [--recall]
        [command, provider_config, second_config, rest @ ..] if command == "probe-tools" => {
            let usage = || io::Error::other("usage: probe-tools PROVIDER_CONFIG SECOND_CONFIG [RUNS of 1 to 20] [--recall]");
            let (rest, recall_only) = match rest {
                [first @ .., last] if last == "--recall" => (first, true),
                _ => (rest, false),
            };
            let runs = match rest {
                [] => 3,
                [runs] => runs.parse().ok().filter(|runs| (1..=20).contains(runs)).ok_or_else(usage)?,
                _ => return Err(usage().into()),
            };
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(
                lamp_voice::second_provider::probe::tools::run(Path::new(provider_config), Path::new(second_config), runs, recall_only),
            )?;
        }
        // probe-cascade PROVIDER_CONFIG SECOND_CONFIG [RUNS] [--look JPEG | --wire [--kept] [--speech] | --wire --socket | --wire --sizes]
        [command, provider_config, second_config, rest @ ..] if command == "probe-cascade" => {
            let (runs, look, wire) = probe_options(rest)?;
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
            if let Some(mode) = wire {
                // The relay's near addresses, read as the voice reads them.
                let routes = runtime.block_on(lamp_voice::second_provider::probe::relay_routes_at(Path::new(second_config)))?;
                // Its requests are made by hand, one at a time, blocking;
                // the chats that build them are made in the runtime.
                let _entered = runtime.enter();
                lamp_voice::second_provider::probe::wire::run(Path::new(provider_config), Path::new(second_config), runs, mode, &routes)?;
            } else {
                runtime.block_on(
                    lamp_voice::second_provider::probe::run(Path::new(provider_config), Path::new(second_config), runs, look.as_deref()),
                )?;
            }
        }
        #[cfg(target_os = "linux")]
        [command, role, dir, parent, worker, mode, flags @ ..] if command == "worker" && role == "ring" && mode == lamp_voice::ring_caps::FLAG => {
            let parent = parse_boot(parent)?;
            let worker = parse_boot(worker)?;
            let (bus_root, flags) = lamp_voice::bus_out::take_root(flags)?;
            if !flags.is_empty() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown capped ring worker option").into());
            }
            let mut channels = WorkerChannels::bind(Path::new(dir), role, parent, worker, true)?;
            channels.connect(Path::new(dir), role, true)?;
            lamp_voice::ring_worker::run_with_caps(channels, parent, bus_root.as_deref())?;
        }
        // worker ring DIR PARENT WORKER --continuous [--resting CEILING] [--bus-root DIR] [--eye] [--head-touch] [--light-requests] [--output-caps]
        #[cfg(target_os = "linux")]
        [command, role, dir, parent, worker, mode, flags @ ..] if command == "worker" && role == "ring" && mode == "--continuous" => {
            let parent = parse_boot(parent)?;
            let worker = parse_boot(worker)?;
            let (ceiling, flags) = match flags {
                [flag, ceiling, rest @ ..] if flag == "--resting" => {
                    let ceiling: u16 = ceiling.parse().map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "resting ceiling must be 0 to 120"))?;
                    (Some(ceiling), rest)
                }
                rest => (None, rest),
            };
            let (bus_root, flags) = lamp_voice::bus_out::take_root(flags)?;
            let (eye, flags) = lamp_voice::ring_sight::take(flags);
            let (eye_strong, flags) = lamp_voice::ring_sight::take_strong(flags);
            let (head_touch, flags) = lamp_voice::ring_head::take(flags);
            let (light_requests, flags) = lamp_voice::ring_light::take(flags);
            let (skill_light_speaking, flags) = lamp_voice::ring_light::take_speaking(flags);
            let (light_keep, flags) = lamp_voice::ring_light::take_keep(flags);
            let (every_pulse, flags) = lamp_voice::ring_light::take_every_pulse(flags);
            let (output_caps, flags) = lamp_voice::ring_caps::take(flags);
            if !flags.is_empty() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown ring worker option").into());
            }
            let mut channels = WorkerChannels::bind(Path::new(dir), role, parent, worker, true)?;
            channels.connect(Path::new(dir), role, true)?;
            lamp_voice::ring_worker::run_continuous(
                channels,
                parent,
                lamp_voice::ring_worker::RingOptions {
                    resting: ceiling,
                    bus_root: bus_root.as_deref(),
                    eye,
                    eye_strong,
                    head_touch,
                    light_requests,
                    skill_light_speaking,
                    light_keep,
                    every_pulse,
                    output_caps,
                },
            )?;
        }
        // worker privacy DIR PARENT WORKER --controls-root ROOT [--no-slide]
        [command,role,dir,parent,worker,flags @ ..] if command=="worker" && role=="privacy" && flags.first().is_some_and(|flag| flag==lamp_voice::privacy_bus::ROOT_FLAG) => {
            let parent=parse_boot(parent)?;
            let worker=parse_boot(worker)?;
            let (root,route,ride)=lamp_voice::privacy_bus::worker_route(flags)?;
            let mut channels=WorkerChannels::bind(Path::new(dir),role,parent,worker,true)?;
            channels.connect(Path::new(dir),role,true)?;
            lamp_voice::privacy_bus::run(channels,parent,&root,route,ride)?;
        },
        #[cfg(target_os="linux")]
        [command,role,dir,parent,worker,mode,flag,root] if command=="worker" && role=="ring" && mode=="--privacy" && flag=="--bus-root" => {
            let parent=parse_boot(parent)?;
            let worker=parse_boot(worker)?;
            let root=Path::new(root);
            if !root.is_absolute() { return Err(io::Error::other("privacy indicator root must be absolute").into()); }
            let mut channels=WorkerChannels::bind(Path::new(dir),role,parent,worker,true)?;
            channels.connect(Path::new(dir),role,true)?;
            lamp_voice::privacy_indicator::run(channels,parent,root)?;
        },
        [command,role,dir,parent,worker] if command=="worker" => {
            let parent=parse_boot(parent)?;
            let worker=parse_boot(worker)?;
            let mut channels=WorkerChannels::bind(Path::new(dir),role,parent,worker,true)?;
            channels.connect(Path::new(dir),role,true)?;
            match role.as_str() {
                #[cfg(target_os="linux")]
                "privacy"=>lamp_voice::physical_privacy::run(channels)?,
                #[cfg(target_os="linux")]
                "ring"=>lamp_voice::ring_worker::run(channels,parent)?,
                "probe"=>probe(channels)?,
                _=>return Err(io::Error::other("worker is not integrated yet").into()),
            }
        },
        _=>return Err(io::Error::new(io::ErrorKind::InvalidInput,"usage: lamp-voice serve PRIVATE_CONFIG.json NEW_OUTPUT [directed options except --diagnostics] | camera-inspect PRIVATE_CAMERA_CONFIG.json SECONDS NEW_REPORT.json | provider-check PRIVATE_CONFIG.json | directed PRIVATE_CONFIG.json SECONDS NEW_OUTPUT [--controls-stop] [--controls-mute ABS_PRIVATE_RECORD] [--controls-bus-root ABS_PATH] [--camera-config ABS_PATH] [--motor-worker ABS_PATH --motor-config ABS_PATH] [--head-qualification-plan ABS_PATH --head-qualification-channel ABS_PRIVATE_DIR] [--failure-notice-dir ABS_PATH --failure-notice-language TAG] [--diagnostics] [--noise-suppression on|off] [--microphone mic1|mic2] [--turn-taking v1-baseline|vad-only|duplex] [--reference-clock fixed|capture] [--reference-recovery off|bounded] [--echo-reference as-given|steady] [--provider-continuation off|kept-start] [--playout-start-ms 0..500 | --playout-continuity] [--cue-socket ABS_PATH] [--ring-channel-ceiling 0..120] [--end-silence-ms 300..2000] [--early-end-ms 100..1000, with --provider-continuation kept-start] | directed-fixture REPLY.wav SHA256 SECONDS NEW_OUTPUT --diagnostics [--fixture-at-ms 1..60000, below SECONDS x 1000, diagnostic only: start the fixture that many ms after listening_ready, with no admitted turn] [--noise-suppression on|off] [--microphone mic1|mic2] [--turn-taking v1-baseline|vad-only|duplex] [--reference-clock fixed|capture] [--reference-recovery off|bounded] [--playout-start-ms 0..500 | --playout-continuity] [--cue-socket ABS_PATH] [--ring-channel-ceiling 0..120] | worker ROLE DIR CONTROLLER_BOOT WORKER_BOOT").into()),
    }
    Ok(())
}

fn probe(mut channels: WorkerChannels) -> Result<()> {
    use lamp_voice::wire::{Control, WorkerEvent};
    channels.control.send(WorkerEvent::Ready)?;
    let mut last = lamp_ipc::monotonic_us();
    loop {
        match channels.control.receive()? {
            Some(Control::Stop) => return Ok(()),
            Some(Control::Authority { .. }) => {
                last = lamp_ipc::monotonic_us();
                channels.control.send(WorkerEvent::Ready)?;
            }
            Some(
                Control::StartCapture
                | Control::ConnectReference { .. }
                | Control::ProviderOutputCapacity { .. },
            ) => {
                return Err(io::Error::other("invalid probe command").into());
            }
            None => {}
        }
        if lamp_ipc::monotonic_us().saturating_sub(last) > 250_000 {
            return Err(io::Error::other("controller heartbeat expired").into());
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[cfg(test)]
mod tests {
    use super::stt_check_options;

    fn options(list: &[&str]) -> Option<(u32, u32)> {
        let list = list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        stt_check_options(&list).ok()
    }

    #[test]
    fn stt_checks_options_are_rounds_then_what_is_held_within_their_bounds() {
        assert_eq!(options(&[]), Some((3, 0)));
        assert_eq!(options(&["5"]), Some((5, 0)));
        assert_eq!(options(&["--held-ms", "600"]), Some((3, 600)));
        assert_eq!(options(&["2", "--held-ms", "1600"]), Some((2, 1_600)));
        for refused in [
            &["0"][..],
            &["21"],
            &["--held-ms", "1601"],
            &["--held-ms"],
            &["2", "--held"],
            &["many"],
        ] {
            assert_eq!(options(refused), None, "{refused:?}");
        }
    }
}
