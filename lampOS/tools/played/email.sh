#!/bin/zsh
# #751's email ask, end to end with a stand-in Harness (account-ask.sh): "did I get any email from the office today?"
# is handed over to the computer's agent, the stand-in answers with two made-up messages, and the lamp tells them
# back. Nothing reaches a real mailbox. PLAYS SOUND IN THE ROOM. argv: LABEL FIXTURE; TASKS=PATH optional (see
# account-ask.sh).
KIND=email
ASK="Hey Lamp, did I get any email from the office today?"
# The stand-in's answer: two made-up messages, with words no one in the room says.
WORDS=(painting expense)
PLAN='{"machines":{"default":true},"new":{"default":true},"show":{"stdout":"{\"id\":\"11111111-1111-4111-8111-111111111111\",\"state\":\"idle\",\"ask\":\"Did I get any email from the office today?\",\"answer\":\"You got two emails from the office today: the facilities team says the Lantern Room is closed for painting on Friday, and payroll says your expense claim for the Hanoi trip was approved.\",\"truncated\":false,\"tools\":2}"}}'
. "${0:A:h}/account-ask.sh"
