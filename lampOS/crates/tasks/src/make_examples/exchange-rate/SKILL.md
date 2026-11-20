---
name: exchange-rate
description: When asked for the exchange rate, it looks up the euro to US dollar rate on the web and says it.
when: When the person asks for the euro to US dollar exchange rate.
made: here
---

# Exchange rate

Said "exchange rate", the lamp fetches the latest euro to US dollar rate from
a public web API and says it in one sentence.

## Uses

- scripts/main.py, no arguments: a public HTTPS API through the run's proxy, then `lamp.say`.

## Result

Nothing is kept or printed: the rate is said.

## Answer

The script says "One euro is 1.08 US dollars right now." with lamp.say, or, when the web can't be reached, "I couldn't reach the exchange rate just now."
