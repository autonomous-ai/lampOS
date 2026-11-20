---
name: agenda-count
description: When I say count my day, the lamp reads my calendar and its agent tells me how many events are left today.
when: When the person asks how many calendar events are left today or how busy their day is.
made: here
---

# Agenda count

A helper script reads today's calendar events left; the lamp's agent sums them up.

## Uses

- scripts/main.py, no arguments: `lamp.service_read("google_calendar", {"what": "events", "from", "to"})` for the rest of today.

## Result

scripts/main.py prints the count and one line per event (its start and title), or why the calendar could not be read.

## Answer

The agent's final answer is only the spoken summary: how many events are left today and when the next one starts, in one or two plain sentences.
