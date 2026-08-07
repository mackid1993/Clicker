# License

RustDVR is Copyright © 2026 David Brustein.

RustDVR is **source available**, not open source. The source can be read, built
and used for any noncommercial purpose. It cannot be sold, redistributed, or
forked. The terms are the PolyForm Strict License 1.0.0, reproduced verbatim
below.

The word **software** in those terms means RustDVR itself: the source code in
this repository and the binaries built from it. It does not mean the third
party components RustDVR is built against, which carry their own licenses and
are covered separately under "Third party components" at the end of this file.

## Exception required by the LGPL

RustDVR is distributed together with libraries licensed under the GNU Lesser
General Public License version 2.1. Section 6 of that license requires that
anyone who receives the combined work be permitted to modify it for their own
use, and to reverse engineer it in order to debug those modifications. The
PolyForm Strict terms below would otherwise forbid exactly that.

Those terms are therefore granted subject to the following exception, which
prevails over anything below that conflicts with it:

> Notwithstanding the restriction on making changes or new works based on the
> software, you are permitted to modify the software for your own use, and to
> reverse engineer it for the purpose of debugging those modifications, solely
> to the extent required by section 6 of the GNU Lesser General Public License
> version 2.1 in respect of the LGPL libraries distributed with it.

This permits private modification and debugging. It does not permit
distribution, and everything else in the terms below is unaffected.

---

# PolyForm Strict License 1.0.0

<https://polyformproject.org/licenses/strict/1.0.0>

## Acceptance

In order to get any license under these terms, you must agree to them as both strict obligations and conditions to all your licenses.

## Copyright License

The licensor grants you a copyright license for the software to do everything you might do with the software that would otherwise infringe the licensor's copyright in it for any permitted purpose, other than distributing the software or making changes or new works based on the software.

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

RustDVR is built against and ships alongside software written by other people.
Those components are **not** licensed under the terms above. Each is governed by
its own license, and where those licenses conflict with the terms above, the
component's own license prevails for that component.

## GStreamer

RustDVR plays video using GStreamer, which is licensed under the GNU Lesser
General Public License version 2.1 or later. GStreamer is **dynamically linked
and shipped unmodified as separate shared libraries**, and is not combined into
RustDVR's own binary.

The LGPL requires that anyone who receives RustDVR be free to modify GStreamer,
to relink RustDVR against their modified version, and to reverse engineer as
necessary to debug that relinking. Nothing in the PolyForm Strict License above
restricts any of that, because GStreamer is not part of "the software" as that
term is used above. Those rights are granted by the LGPL and are not withdrawn
here.

The GStreamer source corresponding to the shipped libraries is available from
<https://gstreamer.freedesktop.org/>. The exact build is pinned in this
repository so it can be reproduced.

Only plugins that declare an LGPL, MIT, BSD or MPL license are shipped. Plugins
wrapping GPL libraries are excluded deliberately: bundling even one would place
the whole distribution under the GPL.

## Rust crates

The Rust dependencies listed in `Cargo.toml`, and their own dependencies, are
licensed by their respective authors, predominantly under MIT and Apache-2.0.
Their terms apply to them, not the terms above.

## Fonts and system components

Segoe UI Variable and Segoe Fluent Icons are Microsoft fonts installed as part
of Windows. RustDVR reads them from the operating system at runtime and does not
redistribute them.

## Channels DVR

RustDVR is an independent client that talks to a Channels DVR server over its
HTTP API. It is not affiliated with, endorsed by, or derived from Channels or
Fancy Bits LLC. No Channels code is used.
