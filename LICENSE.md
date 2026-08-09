# License

Clicker is Copyright © 2026 David Brustein.

Clicker is **source available**. You may read it, build it, run it, change it,
fork it and share it — for any noncommercial purpose. You may not sell it, or
use it to make money. The terms are the PolyForm Noncommercial License 1.0.0,
reproduced verbatim below.

Required Notice: Copyright © 2026 David Brustein
<https://github.com/mackid1993/Clicker>

The word **software** in those terms means Clicker itself: the source code in
this repository and the binaries built from it. It does not mean the third party
components Clicker is built against, which carry their own licenses and are
covered separately under "Third party components" at the end of this file.

## Exception required by the LGPL

Clicker is distributed together with libraries licensed under the GNU Lesser
General Public License version 2.1. Section 6 of that license requires that
anyone who receives the combined work be permitted to modify it for their own
use, and to reverse engineer it in order to debug those modifications.

The terms below already permit changes and new works for any noncommercial
purpose, which satisfies that requirement for anyone using Clicker under them.
What they do not cover is someone whose purpose is commercial, because the terms
below grant such a person nothing at all. So that no recipient is ever left
holding a combined work they are not permitted to debug, those particular rights
are granted unconditionally:

> Notwithstanding anything below, you are permitted to modify the software for
> your own use, and to reverse engineer it for the purpose of debugging those
> modifications, whatever your purpose, solely to the extent required by section
> 6 of the GNU Lesser General Public License version 2.1 in respect of the LGPL
> libraries distributed with it.

This permits private modification and debugging. It grants no right to use
Clicker commercially, and everything else in the terms below is unaffected.

---

# PolyForm Noncommercial License 1.0.0

<https://polyformproject.org/licenses/noncommercial/1.0.0>

## Acceptance

In order to get any license under these terms, you must agree to them as both strict obligations and conditions to all your licenses.

## Copyright License

The licensor grants you a copyright license for the software to do everything you might do with the software that would otherwise infringe the licensor's copyright in it for any permitted purpose.  However, you may only distribute the software according to [Distribution License](#distribution-license) and make changes or new works based on the software according to [Changes and New Works License](#changes-and-new-works-license).

## Distribution License

The licensor grants you an additional copyright license to distribute copies of the software.  Your license to distribute covers distributing the software with changes and new works permitted by [Changes and New Works License](#changes-and-new-works-license).

## Notices

You must ensure that anyone who gets a copy of any part of the software from you also gets a copy of these terms or the URL for them above, as well as copies of any plain-text lines beginning with `Required Notice:` that the licensor provided with the software.  For example:

> Required Notice: Copyright Yoyodyne, Inc. (http://example.com)

## Changes and New Works License

The licensor grants you an additional copyright license to make changes and new works based on the software for any permitted purpose.

## Patent License

The licensor grants you a patent license for the software that covers patent claims the licensor can license, or becomes able to license, that you would infringe by using the software.

## Noncommercial Purposes

Any noncommercial purpose is a permitted purpose.

## Personal Uses

Personal use for research, experiment, and testing for the benefit of public knowledge, personal study, private entertainment, hobby projects, amateur pursuits, or religious observance, without any anticipated commercial application, is use for a permitted purpose.

## Noncommercial Organizations

Use by any charitable organization, educational institution, public research organization, public safety or health organization, environmental protection organization, or government institution is use for a permitted purpose regardless of the source of funding or obligations resulting from the funding.

## Fair Use

You may have "fair use" rights for the software under the law. These terms do not limit them.

## No Other Rights

These terms do not allow you to sublicense or transfer any of your licenses to anyone else, or prevent the licensor from granting licenses to anyone else.  These terms do not imply any other licenses.

## Patent Defense

If you make any written claim that the software infringes or contributes to infringement of any patent, your patent license for the software granted under these terms ends immediately. If your company makes such a claim, your patent license ends immediately for work on behalf of your company.

## Violations

The first time you are notified in writing that you have violated any of these terms, or done anything with the software not covered by your licenses, your licenses can nonetheless continue if you come into full compliance with these terms, and take practical steps to correct past violations, within 32 days of receiving notice.  Otherwise, all your licenses end immediately.

## No Liability

***As far as the law allows, the software comes as is, without any warranty or condition, and the licensor will not be liable to you for any damages arising out of these terms or the use or nature of the software, under any kind of legal claim.***

## Definitions

The **licensor** is the individual or entity offering these terms, and the **software** is the software the licensor makes available under these terms.

**You** refers to the individual or entity agreeing to these terms.

**Your company** is any legal entity, sole proprietorship, or other kind of organization that you work for, plus all organizations that have control over, are under the control of, or are under common control with that organization.  **Control** means ownership of substantially all the assets of an entity, or the power to direct its management and policies by vote, contract, or otherwise.  Control can be direct or indirect.

**Your licenses** are all the licenses granted to you for the software under these terms.

**Use** means anything you do with the software requiring one of your licenses.

---

# Third party components

Clicker is built against and ships alongside software written by other people.
Those components are **not** licensed under the terms above. Each is governed by
its own license, and where those licenses conflict with the terms above, the
component's own license prevails for that component.

## mpv

Clicker plays all video using mpv, which is licensed here under the GNU Lesser
General Public License version 2.1 or later. mpv is **dynamically loaded and
shipped unmodified as a separate shared library**, `libmpv-2.dll`, placed beside
the executable and opened by name at runtime through its public client and
render APIs. It is never folded into Clicker's own binary and never renamed, and
no plugins, scripts or configuration are loaded from anywhere.

mpv is GPL-2.0-or-later by default and is only LGPL when configured that way.
The corresponding source is mpv `v0.41.0`, from
<https://github.com/mpv-player/mpv>, built with `-Dgpl=false -Dlibmpv=true`.
`scripts/build-mpv.ps1` reproduces that build from the pinned tag, and
`build.ps1` reads the license string back out of the finished library and
refuses to package it if it reports GPL.

The libraries mpv itself was linked against ship the same way, beside the
executable, unmodified, each under its own license.

## FFmpeg

FFmpeg does the decoding underneath mpv. It is licensed under the GNU Lesser
General Public License version 2.1 or later, and is **shipped unmodified as
separate shared libraries** — `avcodec`, `avformat`, `avfilter`, `avutil`,
`swscale` and `swresample` — placed beside the executable and loaded at
runtime. It is never folded into Clicker's own binary and never renamed.
Clicker does not link against it directly; it reaches FFmpeg only through mpv.

The LGPL requires that anyone who receives Clicker be free to modify mpv and
FFmpeg, to relink Clicker against their modified versions, and to reverse
engineer as necessary to debug that relinking. Nothing in the PolyForm
Noncommercial License above restricts any of that, because neither is part of
"the software" as that term is used above. Those rights are granted by the LGPL
and are not withdrawn here, and the exception at the top of this file exists so
that they cannot be.

The corresponding source is FFmpeg `n7.1.1`, from
<https://github.com/FFmpeg/FFmpeg>. It is built from that source rather than
downloaded prebuilt, because the license has to be provable: every prebuilt
FFmpeg-bearing media binary for Windows that was examined embeds a build
configured `--enable-gpl`, and shipping one of those inside a noncommercially
licensed application would place the entire distribution under the GPL.

This build is configured `--disable-gpl --disable-nonfree`. The configure line
is recorded inside the libraries themselves and can be read back at runtime
through `av_license()` and `FFMPEG_CONFIGURATION`; Clicker prints it on startup
and shows it in Settings under About. `scripts/build-mpv.ps1` reproduces the
build from the pinned tag, and `build.ps1` re-reads the shipped library and
refuses to package it if it reports GPL.

## Rust crates

The Rust dependencies listed in `Cargo.toml`, and their own dependencies, are
licensed by their respective authors, predominantly under MIT and Apache-2.0.
Their terms apply to them, not the terms above.

## Fonts and system components

Segoe UI Variable and Segoe Fluent Icons are Microsoft fonts installed as part
of Windows. Clicker reads them from the operating system at runtime and does not
redistribute them.

## Channels DVR

Clicker is an independent, unofficial client that talks to a Channels DVR server
over its public HTTP API. It is not affiliated with, endorsed by, sponsored by,
supported by, or derived from Channels or Fancy Bits, LLC. No Channels code is
used. "Channels" and "Channels DVR" are the property of their respective owners
and appear here only to identify the server this program communicates with.
