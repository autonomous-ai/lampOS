#!/bin/zsh
# #751's calendar ask, end to end with a stand-in Harness (account-ask.sh): "what's on my calendar today?" is handed
# over to the computer's agent, the stand-in answers with two made-up events, and the lamp tells them back. Nothing
# reaches a real calendar. PLAYS SOUND IN THE ROOM. argv: LABEL FIXTURE; TASKS=PATH optional (see account-ask.sh).
KIND=calendar
ASK="Hey Lamp, what's on my calendar today?"
# The stand-in's answer: two made-up events, with words no one in the room says.
WORDS=(lantern dentist)
PLAN='{"machines":{"default":true},"new":{"default":true},"show":{"stdout":"{\"id\":\"11111111-1111-4111-8111-111111111111\",\"state\":\"idle\",\"ask\":\"What is on my calendar today?\",\"answer\":\"You have two things on your calendar today: at 10:00 a design review in the Lantern Room for 45 minutes, and at 15:30 a dentist appointment.\",\"truncated\":false,\"tools\":2}"}}'
. "${0:A:h}/account-ask.sh"
