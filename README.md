# Gemino
**A fast predictable MPMC channel for rust!**

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
and offers both a blocking and asynchronous API which can be used at the same time. The design of the underlying data structure 
was inspired by [LMAX Disruptor](https://lmax-exchange.github.io/disruptor/) but is simpler and has slightly fewer constraints.

Gemino was developed as a personal learning exercise, but I believe it is good enough to share and use.
> Gemino is still in very early development, is lacking solid testing and makes use of unsafe. Use at your own risk!

## Why is it fast?
1. It's built on a ring buffer which allows essentially wait free writes and wait free reads where data is available.
2. It's "Lock-free". Gemino uses 2 atomic variables to handle synchronisation of senders and receivers.  
3. Relaxed constraints. There is no guaranteed delivery of messages over gemino channels. If a receiver doesn't keep up it will miss out

## Why use Gemino?

* You need a very fast buffered channel.
* You need both async and blocking access to the same channel.
* You need multiple receiver and sender but are not concerned with data loss if a receiver falls behind.
* You need to be able to close the channel and have all other senders and receivers be notified.
* Your application can make use of bulk reads from receivers (bulk reads are an extremely fast (and memory efficient) way to read messages)

## Why not use Gemino?

* Gemino uses unsafe and may exhibit undefined behaviour that we haven't seen or found yet.
* If you need to absolutely guarantee that all receivers get all the messages without fail Gemino is not for you.

### Benchmarks
Coming soon