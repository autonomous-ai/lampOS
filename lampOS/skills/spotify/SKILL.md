---
name: spotify
description: Call when someone asks the lamp to play, pause, resume or skip their music on Spotify - "play Clair de Lune on Spotify", "put on some Debussy", "pause the music", "next song", "go back one". Give the action they asked for. Only when asked.
---

# Spotify

Ask the lamp to play a song, an artist, an album or a playlist, to pause,
to go on, or to skip forward or back, and your own Spotify does it on the
device you have it open on: your phone, your computer or a speaker. The
lamp does not become a speaker itself, and it never starts music on its
own.

It needs:

- Spotify Premium (Spotify plays from another device only for Premium);
- your Spotify linked in the app (Skills, then Connectors), allowing
  playback;
- Spotify open on a device, so there is something to play on.

Only the lamp's owner can use it, heard at the lamp, as for every linked
service. Asked to play something, it plays at once: there is no read-back,
as there is before a mail is sent. If Spotify is closed everywhere, the
lamp says to open it on your phone or speaker first; without Premium, it
says that playing needs Premium.

The playing is the lamp's linked-services part's (`service_play`,
`docs/services.md`, "Spotify: playback"), behind `LAMP_SERVICES_PLAYBACK`,
which is off in the product until it has been tried on a lamp. With this
skill on and playback on, the lamp's voice uses that function in this
skill's place; with playback off, this skill says the lamp can't play
Spotify yet.

If asked about it, the lamp says something like: "Ask me to play a song,
an artist or a playlist on your Spotify, or to pause or skip, and it plays
on your phone or speaker."
