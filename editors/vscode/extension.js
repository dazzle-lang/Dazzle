// The Dazzle extension: registers the language, runs `dz lsp` (spec §17.4), and runs
// projects.
//
// Everything that understands Dazzle lives in the compiler. This file only finds the `dz`
// binary, starts it, restarts it when it is rebuilt, and hands `dz run` to a task.
'use strict';

const fs = require('fs');
const path = require('path');
const vscode = require('vscode');
const { LanguageClient, TransportKind } = require('vscode-languageclient/node');

const EXE = process.platform === 'win32' ? 'dz.exe' : 'dz';

let client;
let output;
let rebuildWatcher;

/** The `dz` to run: the setting, then a build in a workspace folder, then `PATH`. */
function locateServer() {
  const configured = vscode.workspace.getConfiguration('dazzle').get('server.path');
  if (configured) {
    const folder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? '';
    return configured.replace('${workspaceFolder}', folder);
  }
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    for (const profile of ['debug', 'release']) {
      const candidate = path.join(folder.uri.fsPath, 'target', profile, EXE);
      if (fs.existsSync(candidate)) return candidate;
    }
  }
  return 'dz';
}

/**
 * Run a copy of the binary rather than the binary itself. Windows will not let a running
 * executable be replaced, so running target/debug/dz.exe directly would make every
 * `cargo build` of the compiler fail for as long as the editor is open.
 */
function stage(context, source) {
  if (!path.isAbsolute(source) || !fs.existsSync(source)) return source;
  try {
    const dir = context.globalStorageUri.fsPath;
    fs.mkdirSync(dir, { recursive: true });
    for (const old of fs.readdirSync(dir)) {
      if (!old.startsWith('dz-server-')) continue;
      try {
        fs.unlinkSync(path.join(dir, old));
      } catch {
        // Still running in another window; it is cleaned up next time.
      }
    }
    const staged = path.join(dir, `dz-server-${Date.now()}${path.extname(source)}`);
    fs.copyFileSync(source, staged);
    return staged;
  } catch (e) {
    output.appendLine(`could not copy ${source} (${e.message}); running it in place`);
    return source;
  }
}

async function start(context) {
  const source = locateServer();
  const command = stage(context, source);
  output.appendLine(command === source ? `starting ${source}` : `starting ${source} (as ${command})`);

  client = new LanguageClient(
    'dazzle',
    'Dazzle',
    { command, args: ['lsp'], transport: TransportKind.stdio },
    {
      documentSelector: [{ scheme: 'file', language: 'dazzle' }],
      outputChannel: output,
      synchronize: {
        // A new file or a changed manifest can change what a project contains.
        fileEvents: vscode.workspace.createFileSystemWatcher('**/{dazzle.toml,*.dz}'),
      },
    },
  );
  try {
    await client.start();
  } catch (e) {
    client = undefined;
    vscode.window.showErrorMessage(
      `Dazzle: could not start the language server "${source}": ${e.message ?? e}. ` +
        'Build the compiler with `cargo build`, or set "dazzle.server.path".',
    );
  }
  watchForRebuild(context, source);
}

async function restart(context) {
  if (client) {
    const old = client;
    client = undefined;
    try {
      await old.stop();
    } catch {
      // It may already have exited.
    }
  }
  await start(context);
}

/** Restart when `cargo build` replaces the binary, so the editor always runs the latest compiler. */
function watchForRebuild(context, source) {
  rebuildWatcher?.close();
  rebuildWatcher = undefined;
  if (!vscode.workspace.getConfiguration('dazzle').get('server.restartOnRebuild')) return;
  if (!path.isAbsolute(source) || !fs.existsSync(source)) return;
  let timer;
  try {
    rebuildWatcher = fs.watch(path.dirname(source), (_event, name) => {
      if (name !== path.basename(source)) return;
      clearTimeout(timer);
      // The linker writes the binary in several steps; wait for it to settle.
      timer = setTimeout(() => {
        output.appendLine(`${source} changed; restarting`);
        restart(context);
      }, 1000);
    });
  } catch (e) {
    output.appendLine(`not watching ${source} for rebuilds: ${e.message}`);
  }
}

/** The project a file belongs to: the nearest directory above it holding a `dazzle.toml`. */
function projectOf(file) {
  let dir = path.dirname(file);
  for (;;) {
    if (fs.existsSync(path.join(dir, 'dazzle.toml'))) return dir;
    const up = path.dirname(dir);
    if (up === dir) return undefined;
    dir = up;
  }
}

/**
 * `dz run` or `dz check` on the active file's project, as a task: output goes to the
 * terminal panel, and the task can be re-run from there. The original binary is used, not
 * the server's staged copy, so a freshly built compiler is picked up immediately.
 */
async function runProject(command) {
  const editor = vscode.window.activeTextEditor;
  const document = editor?.document;
  const project = document && document.uri.scheme === 'file' ? projectOf(document.uri.fsPath) : undefined;
  if (!project) {
    vscode.window.showErrorMessage('Dazzle: this file is not part of a project (there is no dazzle.toml above it).');
    return;
  }
  // `dz` reads the files on disk.
  await vscode.workspace.saveAll(false);
  const scope = vscode.workspace.getWorkspaceFolder(document.uri) ?? vscode.TaskScope.Workspace;
  const task = new vscode.Task(
    { type: 'dazzle', command, project },
    scope,
    `${command} ${path.basename(project)}`,
    'dazzle',
    new vscode.ProcessExecution(locateServer(), [command, project], { cwd: project }),
  );
  task.presentationOptions = {
    reveal: vscode.TaskRevealKind.Always,
    panel: vscode.TaskPanelKind.Dedicated,
    clear: true,
    focus: false,
  };
  await vscode.tasks.executeTask(task);
}

function activate(context) {
  output = vscode.window.createOutputChannel('Dazzle');
  context.subscriptions.push(
    output,
    vscode.commands.registerCommand('dazzle.run', () => runProject('run')),
    vscode.commands.registerCommand('dazzle.check', () => runProject('check')),
    vscode.commands.registerCommand('dazzle.restartServer', () => restart(context)),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration('dazzle.server')) restart(context);
    }),
  );
  return start(context);
}

async function deactivate() {
  rebuildWatcher?.close();
  if (client) await client.stop();
}

module.exports = { activate, deactivate };
