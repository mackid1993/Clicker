# Changelog

## 1.2.0

**Clicker runs on macOS and Linux.** One codebase, one interface — the same
Fluent surface everywhere — over a new `platform` module that holds the
handful of things three operating systems genuinely do differently: where
files live, which fonts to read, how libmpv and OpenGL are found, how the
live buffer gives disk back, and what the clock thinks local time is.

On a Mac the window is a Mac window: native traffic lights float over the
same dark surface, the system draws the corners and the shadow and handles
edge resizing, and everything inside is unchanged. Apple silicon only.
`scripts/build-macos.sh` produces the .app; libmpv comes from
`brew install mpv`. On Linux it is a .deb and a source build — `make deps &&
make && sudo make install` on anything the .deb does not cover — both built
against a player compiled from the same pinned mpv tag and the same LGPL-only
flags as the Windows installer.

**Video on Linux renders on a thread of its own.** A driver that makes the
caller wait — anywhere OpenGL is translated rather than native — was holding
the interface's own thread inside a render call for most of a frame interval,
which is a window that stops answering the mouse and a picture that sheds half
its frames while every counter reads healthy. mpv now draws on a worker with
its own context and hands over only frames the GPU has finished, which is how
mpv's own player survives the same drivers. Against a 1080p60 channel that is
60fps with one dropped frame in a minute, where the interface-thread path
managed 50 and dropped frames throughout. `CLICKER_RENDER_THREAD=0` puts it
back on the interface thread.

Text on Linux falls back to a face the machine actually has, asked of
fontconfig rather than guessed at by path, so glyphs egui's bundled font does
not carry — the arrow in the stats card's seek range, among others — draw as
themselves instead of as an empty box.

The icon glyphs on the new platforms come from Microsoft's MIT-licensed
Fluent UI System Icons, subset to the twenty-eight glyphs the interface
draws and bundled into the binary; Windows keeps reading Segoe Fluent Icons
from the system it ships with. The client-name default now asks POSIX for
the hostname instead of a Windows environment variable, and the settings
screen only offers the notification-area option where a notification area
exists.

Windows is unchanged, deliberately and verifiably: the same code moved
behind the platform seam, the same codepoints, the same fonts, and a CI
check that compiles all three platforms before anything merges.

## 1.1.6

**The library can be sorted, not just searched.** A sort menu stands beside
the search box, and each tab offers the orders that mean something for what
it shows. Films sort by name in either direction, by year in either
direction, by what arrived most recently, and by length. Series sort by
name, by unwatched episodes, by how many recordings they hold, and by which
recorded something most recently. Each tab keeps its own choice, and the
choice is remembered across launches on the same reasoning as the guide's
collection: picking a sort is picking the default.

Films without a year sort to the end of the year orders rather than the
front — a missing year is not a claim to be the newest thing in the library.
The year under a film's poster now prefers the release year over the air
date, which is what the year orders use, so the label and the position
always agree.

The Recorded tab and the downloads screen sort too, through the same menu.
Recordings open newest first — what a recording list has always meant — and
can be turned by name or by length instead. Downloads open with whatever is
moving on top and can be turned by name. The schedule stays chronological on
purpose: a list of what happens next sorted by anything but when is not a
schedule.

Scrolling the TV grid does slightly less work than before: the recording
count under each poster now comes from one pass over the library instead of
one scan of every recording per visible row.

## 1.1.5

**Artwork stops flickering in a large library — the Movies tab, this time.**
The two previous fixes were real, but both stopped at the TV grid. Movies
still built every card on every frame, and the cache rule meant to protect
what is on screen was aging each request separately, so it protected exactly
one poster — the last one asked for — and evicted the rest of the screen,
which was requested again at once. Past about three hundred movies that is a
rolling wave of posters vanishing and returning. The Movies tab now builds
only the rows on screen through the same code as the TV grid, and everything
drawn in a frame ages as one, so nothing visible is ever evicted.

The diagnostic line the library writes to the log now covers the Movies tab
too, and says which grid it is reporting on. Its silence there is how this
went undiagnosed across two releases: the log said the library was healthy,
and it was — the half of it that could speak.

**Artwork stops flickering in a large library.** Every series was being drawn
every frame, on screen or not, so a library of five hundred asked for five
hundred pictures sixty times a second and no cache could hold them. Only what
is visible is built now. Scrolling a large library is quicker for the same
reason: the count under each poster was also being worked out for rows nobody
could see.

**Caches and logs can be moved.** A third folder setting, beside downloads and
the live buffer, covering the guide cache, the library cache, the player log
and the crash log. The guide cache alone is around twenty-five megabytes a day,
and none of it has any reason to be on the system drive.

**A server with no version reported no longer shows "vunknown".**

The library page now writes a line to the log every so often saying how many
series it built against how many exist and how much artwork is in memory, so a
report of flickering artwork can be read rather than guessed at.

## 1.1.3

**Artwork stops flickering in a large library.** Posters would appear, vanish
and come back on a screen nobody was scrolling. Two things caused it. Artwork
was fetched far larger than it is ever drawn, so far less of it fitted in
memory than should have. And when memory ran short, the rule for choosing what
to drop could pick the pictures currently on screen, which were then requested
again at once, pushing out others in turn. Cards are now fetched at the size
they are shown, and nothing on screen is discarded.

## 1.1.2

**A black rectangle no longer flashes over the picture.** The buffer the video
is drawn into is created empty, and a new one is made at the start of a file
and again whenever the picture changes size partway through. It was being put
on screen before there was anything in it.

**Continue Watching shows everything it should.** Some part-watched recordings
were being left out of the list.

**The closed caption button works on recordings.** Captions carried inside the
video only become a track once decoding has actually reached some, which is
well after a file is opened. The button was asking for whichever track was
chosen at the start, and at the start there was none, so pressing it did
nothing. It now finds the caption track and selects it.

## 1.1.1

Four fixes from a user report.

**Recordings no longer play as colored hash.** The picture and the interface
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

**The picture never leaves the graphics chip.** Decoding, color conversion
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
feature. It is now drawn from the whole library, labeled as a pick, and deals
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
