# Gemino
**A MPMC channel**

[![Crates.io][crates-badge]][crates-url]
[![Documentation][doc-badge]][doc-url]
[![MIT licensed][mit-badge]][mit-url]

[crates-badge]: https://img.shields.io/crates/v/gemino.svg
[crates-url]: https://crates.io/crates/gemino
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: https://github.com/lochlanna/gemino/blob/main/LICENSE
[doc-badge]: https://docs.rs/gemino/badge.svg
[doc-url]: https://docs.rs/gemino

## What is Gemino
Gemino is an implementation of a multi producer multi consumer (MPMC) broadcasting channel where all receivers have the 
opportunity to receive all messages sent by all producers. The library has been designed with speed and ease of use in mind
and offers both a blocking and asynchronous API which can be used at the same time.

Gemino was developed as a personal learning exercise, but I believe it is good enough to share and use.
> Gemino is still in very early development, is lacking rigorous testing and makes use of unsafe as well as unstable features. Use at your own risk!

## Unstable Features
Gemino makes use of the unstable [specialization](https://rust-lang.github.io/rfcs/1210-impl-specialization.html) feature which is currently
marked as incomplete due to soundness issues. Geminos use of this feature is basic enough that it avoids the soundness issues
and probably isn't using anything that is likely to change or misbehave.
This RFC has been open since 2015 and doesn't have any real timeline on when or if this feature might become stable.

## Why use Gemino?
* You need both async and blocking access to the same channel.
* You need multiple receivers and senders but are not concerned with data loss if a receiver falls behind.
* You need to be able to close the channel and have all parties be notified.

## Why not use Gemino?
* Unstable nightly only features are something you want to avoid.
* Gemino uses unsafe and may exhibit undefined behaviour that we haven't seen or found yet.
* If you need to absolutely guarantee that all receivers get all the messages without fail Gemino is not for you.

### Benchmarks
Coming soon