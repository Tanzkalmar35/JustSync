import * as vscode from "vscode";
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let statusBarItem: vscode.StatusBarItem;
let cursorDecoration: vscode.TextEditorDecorationType;
let selectionChangeListener: vscode.Disposable | undefined;

// Map of agent_id -> { uri: string, range: vscode.Range }
const remoteCursors = new Map<string, { uri: string, range: vscode.Range }>();

const SERVER_PATH = "just_sync";

interface RemoteCursorParams {
    agent_id: string;
    uri: string;
    position: {
        line: number;
        character: number;
    };
}

export function activate(context: vscode.ExtensionContext) {
    console.log(">> JustSync Extension Active");

    // Create a decoration type for the remote cursor
    cursorDecoration = vscode.window.createTextEditorDecorationType({
        backgroundColor: "rgba(49, 116, 143, 0.5)", // Semi-transparent blue
        border: "1px solid #31748f",
        after: {
            contentText: "┃",
            color: "#31748f",
            fontWeight: "bold",
        },
    });

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
        args = ["--mode", "host"];
        modeLabel = "Host";

        const relay_addr = await vscode.window.showInputBox({ title: "Relay server addr" });
        if (!relay_addr) return;

        args.push("--remote-ip");
        args.push(relay_addr);

        const password = await vscode.window.showInputBox({ title: "Password to use" });
        if (!password) return;

        args.push("--key");
        args.push(password);
    } else {
        args = ["--mode", "peer"];
        modeLabel = "Peer";

        const relay_addr = await vscode.window.showInputBox({ title: "Relay server addr" });
        if (!relay_addr) return;

        args.push("--remote-ip");
        args.push(relay_addr);

        const session_name = await vscode.window.showInputBox({ title: "Session name" });
        if (!session_name) return;

        args.push("--session-name");
        args.push(session_name);

        const password = await vscode.window.showInputBox({ title: "Password to use" });
        if (!password) return;

        args.push("--key");
        args.push(password);
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

        // Handle remote cursor notifications from the server
        client.onNotification("$/justsync/remoteCursor", (params: RemoteCursorParams) => {
            handleRemoteCursor(params);
        });

        // Track local cursor movements to send to peers
        setupLocalCursorTracking();

        updateStatusBar(true, modeLabel);
        vscode.window.showInformationMessage(`JustSync Started (${modeLabel})`);
    } catch (e) {
        vscode.window.showErrorMessage(`Failed to start JustSync: ${e}`);
        updateStatusBar(false);
    }
}

function setupLocalCursorTracking() {
    if (selectionChangeListener) {
        selectionChangeListener.dispose();
    }

    selectionChangeListener = vscode.window.onDidChangeTextEditorSelection((e) => {
        if (client && client.isRunning()) {
            const position = e.selections[0].active;
            const params = {
                textDocument: { uri: e.textEditor.document.uri.toString() },
                position: {
                    line: position.line,
                    character: position.character
                }
            };
            client.sendNotification("$/justsync/cursor", params);
        }
    });
}

function handleRemoteCursor(params: RemoteCursorParams) {
    const pos = new vscode.Position(params.position.line, params.position.character);
    const range = new vscode.Range(pos, pos);
    
    // Store the new position
    remoteCursors.set(params.agent_id, { uri: params.uri, range });

    // Update decorations for all visible editors
    updateAllRemoteCursors();
}

function updateAllRemoteCursors() {
    for (const editor of vscode.window.visibleTextEditors) {
        const uri = editor.document.uri.toString();
        const ranges = Array.from(remoteCursors.values())
            .filter(c => c.uri === uri)
            .map(c => c.range);
        
        editor.setDecorations(cursorDecoration, ranges);
    }
}

async function stopClient() {
    if (selectionChangeListener) {
        selectionChangeListener.dispose();
        selectionChangeListener = undefined;
    }

    // Clear local state
    remoteCursors.clear();

    // Clear all remote cursor decorations
    vscode.window.visibleTextEditors.forEach(editor => {
        editor.setDecorations(cursorDecoration, []);
    });

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
        );
    } else {
        statusBarItem.text = `$(play) JustSync`;
        statusBarItem.tooltip = "Click to Start Host/Join";
        statusBarItem.backgroundColor = undefined;
    }
}
