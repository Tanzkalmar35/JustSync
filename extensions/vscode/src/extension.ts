import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let statusBarItem: vscode.StatusBarItem;

// Point this to your global binary or absolute path
const SERVER_PATH = "JustSync";

export function activate(context: vscode.ExtensionContext) {
  console.log(">> JustSync Extension Active");

  // 1. Create Status Bar Button
  statusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    100,
  );
  statusBarItem.command = "justsync.toggle";
  context.subscriptions.push(statusBarItem);

  // 2. Register the Command
  const commandId = "justsync.toggle";
  context.subscriptions.push(
    vscode.commands.registerCommand(commandId, async () => {
      if (client && client.isRunning()) {
        await stopClient();
      } else {
        await showStartMenu();
      }
    }),
  );

  // 3. Initial UI State
  updateStatusBar(false);
  statusBarItem.show();
}

export function deactivate(): Thenable<void> | undefined {
  return stopClient();
}

// --- Helper Functions ---

async function showStartMenu() {
  const selection = await vscode.window.showQuickPick(
    ["Host (Port 4444)", "Join (127.0.0.1:4444)"],
    { placeHolder: "Start JustSync..." },
  );

  if (!selection) return;

  let args: string[] = [];
  let modeLabel = "";

  if (selection.startsWith("Host")) {
    args = ["--mode", "host", "--port", "4444"];
    modeLabel = "Host";
  } else {
    args = ["--mode", "peer", "--remote-ip", "127.0.0.1"];
    modeLabel = "Peer";
  }

  startClient(args, modeLabel);
}

async function startClient(args: string[], modeLabel: string) {
  const serverOptions: ServerOptions = {
    run: { command: SERVER_PATH, args: args, transport: TransportKind.stdio },
    debug: { command: SERVER_PATH, args: args, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "*" }],
    // Prevent VS Code from complaining if the server crashes during dev
    errorHandler: {
      error: () => ({ action: 2 }), // Shutdown
      closed: () => ({ action: 2 }), // Do not restart
    },
  };

  client = new LanguageClient(
    "justsync",
    "JustSync Client",
    serverOptions,
    clientOptions,
  );

  try {
    await client.start();
    updateStatusBar(true, modeLabel);
    vscode.window.showInformationMessage(`JustSync Started (${modeLabel})`);
  } catch (e) {
    vscode.window.showErrorMessage(`Failed to start JustSync: ${e}`);
    updateStatusBar(false);
  }
}

async function stopClient() {
  if (!client) return;

  try {
    await client.stop();
  } catch (e) {
    // Ignore stop errors
  } finally {
    client = undefined;
    updateStatusBar(false);
    vscode.window.showInformationMessage("JustSync Stopped");
  }
}

function updateStatusBar(running: boolean, info?: string) {
  if (running) {
    statusBarItem.text = `$(radio-tower) JustSync: ${info}`;
    statusBarItem.tooltip = "Click to Stop JustSync";
    statusBarItem.backgroundColor = new vscode.ThemeColor(
      "statusBarItem.warningBackground",
    ); // Orange
  } else {
    statusBarItem.text = `$(play) JustSync`;
    statusBarItem.tooltip = "Click to Start Host/Join";
    statusBarItem.backgroundColor = undefined;
  }
}
