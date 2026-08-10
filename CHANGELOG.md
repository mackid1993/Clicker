# Changelog

## 1.1.1

Four fixes from a user report.

**Recordings no longer play as coloured hash.** The picture and the interface
are drawn by two different things sharing one graphics context, and the
settings that say how image data is laid out in memory are shared with it.
Whichever drew last left its own settings behind, so video frames were read at
the wrong stride and came out as horizontal noise — from a decoder that had
just decoded them perfectly, which is why nothing anywhere reported a fault.

**Live TV no longer stops every twenty seconds.** At original quality the live
stream is written to a file on disk so that pause and rewind work, and playback
reads that file while it is still being written. It was being read as an
ordinary file, so every time playback caught up with the writer it found the
end of the file and stopped. It now follows the file the way `tail -f` does,
waiting for more instead of stopping. Tuning is also quicker, because the wait
before playback starts existed to work around the same thing.

**Closed captions work.** CEA-608 and CEA-708 captions ride inside the video
rather than in a track of their own, and the player was not being asked to
build a track from them, so the CC button had nothing to turn on. They are also
drawn the way a television draws them — white monospaced text in a solid black
band — rather than as outlined film subtitles, which disappear over a bright
sky.

**The Record button is gone from recordings.** It was shown on anything played
in a wide enough window, including recordings, where recording is either
meaningless or a way to schedule something nobody asked for. It belongs to live
television and is now only there.

## 1.1.0

Clicker plays through mpv now, the picture stays on the graphics chip, memory
is down by two thirds, and the home screen stopped lying about what you were
watching.

### mpv is the video player

Everything Clicker plays goes through **mpv**, built from source and shipped
beside the application. There is no setting for it and no second player to
fall back to.

The reason is edge cases. Broadcast television is full of them — timestamps
that jump at a segment boundary, damaged segments, containers assembled by
something unusual, streams that stop and start — and mpv has been having those
solved for twenty years. Nearly every playback report is one of them, and most
arrive attached to a recording that plays perfectly everywhere else.

**The picture never leaves the graphics chip.** Decoding, colour conversion
and drawing all happen on the integrated GPU, and frames make no round trip
through system memory on the way to the screen. Measured on a 1080p60
recording, video costs about eight percent of one processor core. The discrete
card is still never asked for.

**A log file.** The player has always reported what it was doing, but a
windowed build has no console, so every one of those lines went nowhere. They
now go to `player.log` beside the settings, and Settings has a button that
opens the folder — and mpv's own account of a file goes in there too. If
something stutters, send that file.

**Stats.** Ctrl+I shows what playback is actually doing: frames dropped, A/V
sync, how much is buffered, and what the renderer costs.

mpv and FFmpeg are both LGPL, both built from source with no GPL components,
and both ship as ordinary replaceable libraries. Settings names them under
About, and `licenses/THIRD_PARTY.md` has the full accounting.

### Memory

Idle went from 645MB to **173MB**, and startup from 726MB to the same figure.
Most of it was structural:

- The old player and everything under it is gone, along with its audio stack
  and its own copy of FFmpeg's headers and import libraries. mpv brings one of
  each.
- Drawing moved to OpenGL. The previous graphics backend defaults to trading
  memory for speed and commits large heaps up front; nothing here needs that
  bargain.
- Video frames are no longer copied into system memory and converted before
  being drawn. At 1080p that was eight megabytes of allocation per frame,
  sixty times a second.
- A list of every recording the DVR did not record, cloned in full, held in
  memory and written to the cache file, and read by nothing.
- The server's raw object for every airing in the guide was kept resident to
  serve an action that happens once in a while. A day of listings is 24MB of
  JSON across thirteen thousand airings. It lives on disk now and the one
  being scheduled is read back when it is needed.

### Home screen

**Continue Watching is in the right order.** It was sorted on a field the
server also bumps for its own housekeeping, so a documentary nobody had got 4%
into outranked an episode somebody was a third of the way through. The API has
a last watched timestamp and this client was ignoring it.

**The hero is a shuffle, and says so.** It sits where continue-watching goes,
so showing something that was not what you last watched read as a broken
feature. It is now drawn from the whole library, labelled as a pick, and deals
a new card every time you arrive at the home screen, including coming back
from something you were watching. It carries the season and episode, year,
length, rating, genres, description, director and cast, and its artwork is
fetched at the size it is actually drawn.

### Library and guide

- Shuffle an episode of any series, preferring ones you have not seen.
- Sources are named what Channels names them. A channel carries a device id,
  not a source name, and the guide was showing the id.
- The guide reaches a full day ahead instead of a few hours.
- Listings dated centuries in the future are ignored. Guide data occasionally
  contains jokes.

### Deleting

Every delete asks first, including removing a download and clearing all
finished downloads, which was the most destructive button on the screen and
had no confirmation at all. Deletes go to the DVR's Trash and always did, so a
mistake is recoverable from the server's own admin page.

### Settings

- **Folders.** Downloads and the live buffer can each live wherever you want,
  chosen from a folder picker. Neither is small and both defaulted to the
  system drive.
- **Keyboard.** Every shortcut can be rebound, cleared, or reset, and they can
  all be turned off with one key that keeps working while they are off. The
  whole list is on the settings page, generated from the same table the
  handler reads.
- **Software decoding.** Decoding uses the integrated graphics chip by
  default. Turn this on if a driver produces a picture with artifacts. The
  discrete card is never asked for either way.

### Appearance

- **Full screen fills the screen.** A maximized window is confined to the area
  above the taskbar, and going full screen from there stayed confined to it,
  leaving a strip of desktop along the bottom.
- A new icon: a 1980s remote, drawn at nine sizes and simplified as it gets
  smaller so it still reads at 16 pixels.
- Artwork has rounded corners everywhere. It was being drawn square inside
  rounded borders.
- The hero's gradient is a gradient rather than twenty-four flat bands.
- The library's search box matches the guide's.
- The window reopens where it was left, at the size it was, maximized if it
  was maximized.

## 1.0.0

First release. Windows 10 1809 and up.
