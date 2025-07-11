Hi there!

You're acting as a consultant/senior developer for me during this project. Something about me:

I am a Software Engineering student from Germany in my first year. I work as a full stack junior developer while studying since nearly 3 years, mostly with java, js, ts.
In my free time I learned Rust, Python and Golang. 

I need you to guide me through things like idiomatic code, brainstorming architecture etc. As I am still a student, my first priority is learning. Therefore, please don't provide any code for solving a problem I describe unless I explicitly ask you to. Thanks a lot!

Now a little bit about this project:

This is JustSync. A personal project I write by myself that is supposed to allow for real time code collaboration across editors. So that people like myself can live code in groups and everyone can use their preferred editor for it. For this desire, I'm writing a main engine and small editor extensions.

The editor extensions are just small pieces of code, that do nothing else than (for the beginning) at every file write, send a sync request with the absolute local path to the file to the engine written in Golang. The engine running on localhost then validates that (It checks for actual changes, as if no changes were made, we dont have to sync), and if validated, it either:

- Syncs to all outgoing clients, if the local engine is running host (server) mode
- Syncs to the host, who then syncs it all to all other clients, if running in client mode.

So the engine has 3 modes:

- Host (server) mode, hosting a session
- Client mode, connecting externally to a host session
- admin mode, running on the same machine as the host engine, used for admin actions (we'll come to that).

The networking part:

For the beginning, to build an MVP first, I use http. Later I will use WebSockets, or maybe even when constructing the http infrastructure does not provide the "being simple advantage" anymore.
I have a public domain under my name, lets call that mydomain.com. Then I will use a Cloudflare tunnel that sits at for example sync.mydomain.com, and redirects client traffic to my localhost, where my host engine sits.

Every engine (host or client) runs 2 main goroutines. One for sending out syncs, and one for receiving them.
