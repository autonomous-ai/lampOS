use lamp_voice::attention::{addresses_someone_else_on, called_by_name_on, only_the_name};
fn main() {
    let checks = [
        ("second_route_call", called_by_name_on("Louvre, what did I just ask you?", true)),
        ("live_not_call", !called_by_name_on("Louvre, what did I just ask you?", false)),
        ("second_route_not_other_name", !addresses_someone_else_on("Louvre, what did I just ask you?", true)),
        ("live_keeps_other_name", addresses_someone_else_on("Louvre, what did I just ask you?", false)),
        ("lone_second_route_name", only_the_name("Louvre.")),
        ("lone_live_not_call", !called_by_name_on("Louvre.", false)),
        ("museum_not_call", !called_by_name_on("We went to the Louvre in May.", true)),
        ("museum_not_name_only", !only_the_name("We went to the Louvre in May.")),
        ("unseparated_word_not_call", !called_by_name_on("Louvre what time is it", true)),
        ("greeted_name_call", called_by_name_on("Hey, Louvre. What time is it?", true)),
        ("ordinary_literal_call", called_by_name_on("Lamp, what time is it?", false)),
        ("ordinary_noun_not_call", !called_by_name_on("I left the lamp on.", true)),
    ];
    for (name, pass) in checks { println!("{name}={pass}"); assert!(pass, "{name}"); }
}
