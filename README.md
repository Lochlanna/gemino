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
opportunity to receive all messages sent by all producers. The library has been designed with speed and ease of use in
mind
and offers both a blocking and asynchronous API which can be used at the same time.

If the data being sent through the channel implements Copy there are some nice optimisations that are (automatically)
made which
make the channel significantly faster. When using Copy, no locks are required for reads or writes. It is possible to
detect a corrupt read after it's taken place eliminating the need for locks. This isn't the case with clone where
arbitrary code needs to be run that depends on the memory inside the channel. For this reason locks must be used
to ensure that writers do not overwrite a value that is being cloned. The overhead is high although the overall speed
of the channel is still very fast and with my very rudimentary benchmarks still considerably faster than other
broadcasting channel implementations.

Gemino was developed as a personal learning exercise, but I believe it is good enough to share and use.
> Gemino is still in very early development, is lacking rigorous testing and makes use of unsafe as well as unstable
> features. Use at your own risk!

## Unstable Features

Gemino makes use of the
unstable [`min_specialization`](https://doc.rust-lang.org/beta/unstable-book/language-features/min-specialization.html)
feature. This is the sound subset of the larger and
currently [`specialization`](https://rust-lang.github.io/rfcs/1210-impl-specialization.html) feature.
`min_specialization` is stable enough that it is in use within the standard library. That being said it's still an
unstable feature,
requires nightly.

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