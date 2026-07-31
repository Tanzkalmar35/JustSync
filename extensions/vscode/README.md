# justsync-vscode

The "official" extension for [JustSync](https://github.com/Tanzkalmar35/JustSync) for visual studio code. Provides an
easy to use UI for interacting with the [JustSync binary](https://github.com/Tanzkalmar35/JustSync).

## Installation

The installation is pretty straightforward, as this extension is published to VsCode's marketplace. Just download it from 
there and you're ready to go.

## Usage

Once the installation is complete, you can find a small button on the bottom left corner of the application.

JustSync has 2 modes - Joining as a base peer and joining as a host. Really there is only one difference, that being, 
the hosting peer provides the project for all joining peers. That means the host starts in the project that is to be synced,
while all other peers start in empty directories. Apart from that, all peers are exactly the same.

### Joining as host

Joining as a host means initializing a new session on the [Relay server](https://github.com/Tanzkalmar35/JustSync/tree/master/crates/server).

1. Press the JustSync labelled button in the bottom toolbar. A selection popup window will appear, where you select to host.

2. After that, the extension will ask you for the address of the running relay server

3. Afterwards it will ask you for the session password too

After that, the extension will spawn the just_sync binary, which will connect to the relay server and start a new session there.
A notification should appear with the session name. That's the name you give other peers to join.

### Joining as 'normal' peer

Joining as a 'normal' peer means joining a new session on the [Relay server](https://github.com/Tanzkalmar35/JustSync/tree/master/crates/server).

!! Make sure you start in a completely empty directory !!

1. Press the JustSync labelled button in the bottom toolbar. A selection popup window will appear, where you select peer.

2. After that, the extension will ask you for the address of the running relay server

3. Afterwards it will ask you for the session name and password, both of which you receive from the host.

After that, the extension will spawn the just_sync binary, which will connect to the relay server and join the host's session.
