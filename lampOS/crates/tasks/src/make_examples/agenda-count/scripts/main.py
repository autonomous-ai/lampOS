import asyncio
import datetime

from lampos import Lamp


async def main():
    lamp = await Lamp.local()
    now = datetime.datetime.now().astimezone()
    end = now.replace(hour=23, minute=59, second=59, microsecond=0)
    try:
        result = await lamp.service_read(
            "google_calendar",
            {"what": "events", "from": now.isoformat(), "to": end.isoformat()},
        )
    except Exception as error:
        # Printed for the agent, which says it plainly.
        print(f"The calendar could not be read: {error}")
    else:
        events = result.get("events", []) if isinstance(result, dict) else []
        print(f"{len(events)} event(s) left today.")
        for event in events:
            print(f"{event.get('start', '?')}: {event.get('title', '(no title)')}")
    await lamp.close()


asyncio.run(main())
