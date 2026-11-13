fix(remote-image): images from a URL can no longer reach your local network through older IPv6 ranges

The image-from-URL fetch already refuses loopback, link-local, unique-local and the other private IPv6 ranges. Two ranges that can still route to the local network got through: site-local `fec0::/10` (deprecated, but some LANs still route it) and local-use NAT64 `64:ff9b:1::/48` (RFC 8215, which a network can map to its private IPv4 addresses).

What changed:
- `is_public_v6` treats `fec0::/10` and `64:ff9b:1::/48` as not public, so the fetch refuses them like the other local ranges. The well-known NAT64 prefix `64:ff9b::/96` is still judged by its embedded IPv4 address, as before. The local-use prefix is refused whole, since the IPv4 address can sit at a different offset in it.

Tested:
- Added `fec0::1` and `64:ff9b:1::a00:1` to `addresses_on_this_pc_or_the_local_network_are_refused`.
- `cargo fmt --check`, `cargo test --locked`, and clippy with `-D warnings` for Linux and x86_64-pc-windows-gnu all pass.
- Not tested on a network that actually routes either range.

Thanks for merging the first four so quickly. This is the first of a few more small ones.
