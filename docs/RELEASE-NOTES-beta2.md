# beta2 release notes

Beta2 is a small follow-up on the 26.10 line. It keeps the beta1 desktop behaviour
and packaging channel, and ships two fixes plus a Fedora package rotation.

## Changes you may notice on the desktop

### Floating `resize set` recentres without a jump

`resize set` on a floating window keeps the window's centre, matching sway. The
recentre used to apply through a bare position update, so the old buffer jumped
to the new spot and only then did the resize animation start. A following
`move absolute position` (as in preset-width scripts) slid from that jumped
spot. The recentre now goes through the same animated path as other moves, so
the resize and a chained move read as one continuous motion.

## Packaging

Release artifacts now include Fedora **44** and **45** RPMs. Fedora 43 is no
longer built. Install commands are on
[Getting started](https://github.com/swayward-wm/swayward/wiki/Getting-Started).

## Upstream maintenance

`main` follows niri release tags with a merging-rebase; the 26.10 line lives on
the protected `release/26.10` branch where this tag is cut. See
[Upstream maintenance](UPSTREAM.md).
