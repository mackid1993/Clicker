# Changelog

## 1.1.0

Playback is steadier, the app uses a third less memory, and the home screen
stopped lying about what you were watching.

### Playback

**A read-ahead buffer.** Reading and decoding used to happen in one call on
one thread, so a slow read stopped decoding, and the picture ran out a fifth
of a second later. That was what a stutter was. They are separate threads now
with a queue of compressed packets between them, so a slow read eats into the
buffer instead of into the picture.

The memory goes where it buys the most. Twelve decoded 1080p frames cost 95MB
and buy 0.20 seconds, because a frame is 7.9MB and sixty of them go past every
second. The same memory spent on compressed packets buys around two minutes.
Measured on a real recording: 60 seconds of protection for 48MB, at one
percent of a core.

**Timeline jumps no longer freeze the picture.** Timestamps in a transport
stream are not continuous. A recording made from a segmented source can step
at a segment boundary, and the 33 bit clock wraps every 26.5 hours regardless.
The player waited out the gap, showing nothing, which on a two second segment
is a freeze every two seconds. It now re-anchors and keeps going.

**A log file.** The player has always reported what it was doing, but a
windowed build has no console, so every one of those lines went nowhere. They
now go to `player.log` beside the settings, and Settings has a button that
opens the folder. If something stutters, send that file.

### Memory

Startup dropped from 726MB to 494MB, the peak during library load from 937MB
to 495MB, and idle from 645MB to 404MB. Three causes, none of them
interesting on their own:

- wgpu defaults to a mode that trades memory for speed and commits large heaps
  up front. Nothing here needs that bargain.
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
- **Software decoding.** On by default the decoder uses the integrated
  graphics chip. Turn this on if a driver produces a picture with artifacts.
  The discrete card is never asked for.

### Appearance

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
