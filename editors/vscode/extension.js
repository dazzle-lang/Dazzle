// The Dazzle extension: registers the language, runs `dz lsp` (spec §17.4), and runs and
// debugs projects.
//
// Everything that understands Dazzle lives in the compiler. This file only finds the `dz`
// binary, starts it, restarts it when it is rebuilt, hands `dz run` to a task, and hands a
// `dz build` to a native debugger.
'use strict';

const cp = require('child_process');
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

// --- debugging -----------------------------------------------------------------------------
//
// There is no Dazzle debugger, and none is needed: every generated C++ statement carries a
// `#line` naming its `.dz` source (spec §16.3), so the debug information a C++ compiler
// writes already points at Dazzle files and lines. A `dazzle` launch configuration builds the
// project with `dz build` and starts whichever native debugger fits what the build produced.

/** The project a launch configuration names: a directory, a file in one, or the active file's. */
function projectFor(given) {
  if (given) {
    try {
      if (fs.statSync(given).isDirectory()) {
        return fs.existsSync(path.join(given, 'dazzle.toml')) ? given : projectOf(path.join(given, '_'));
      }
    } catch {
      return undefined;
    }
    return projectOf(given);
  }
  const document = vscode.window.activeTextEditor?.document;
  return document && document.uri.scheme === 'file' ? projectOf(document.uri.fsPath) : undefined;
}

/** `dz build`, returning the executable it reports. Its output goes to the Dazzle channel. */
function buildForDebugging(project) {
  return vscode.window.withProgress(
    { location: vscode.ProgressLocation.Notification, title: `Dazzle: building ${path.basename(project)}` },
    () =>
      new Promise((resolve, reject) => {
        const dz = locateServer();
        output.appendLine(`> ${dz} build ${project}`);
        cp.execFile(dz, ['build', project], { cwd: project, maxBuffer: 64 * 1024 * 1024 }, (err, stdout, stderr) => {
          output.append(stdout);
          output.append(stderr);
          const built = /^built (.+)$/m.exec(stdout);
          if (err || !built) {
            reject(new Error(`the build of ${path.basename(project)} failed`));
            return;
          }
          const exe = built[1].trim();
          resolve(path.isAbsolute(exe) ? exe : path.join(project, exe));
        });
      }),
  );
}

/** `name` on `PATH`, or undefined. */
function onPath(name) {
  const exe = process.platform === 'win32' ? `${name}.exe` : name;
  for (const dir of (process.env.PATH ?? '').split(path.delimiter)) {
    const candidate = path.join(dir, exe);
    if (dir && fs.existsSync(candidate)) return candidate;
  }
  return undefined;
}

/**
 * Which debugger can read what was built. clang++ on Windows writes a PDB beside the
 * executable, which the Visual Studio debugger reads best; g++ writes DWARF, which gdb reads;
 * CodeLLDB reads both and is the fallback. A PDB older than the executable is left over from
 * an earlier build with another compiler, and is not evidence of this one.
 */
function chooseDebugger(exe, wanted) {
  const has = (id) => vscode.extensions.getExtension(id) !== undefined;
  if (wanted && wanted !== 'auto') return wanted;
  if (process.platform === 'win32' && has('ms-vscode.cpptools')) {
    const pdb = exe.replace(/\.exe$/i, '.pdb');
    try {
      if (fs.statSync(pdb).mtimeMs >= fs.statSync(exe).mtimeMs - 10000) return 'vsdbg';
    } catch {
      // No PDB: the build wrote DWARF.
    }
  }
  if (process.platform !== 'darwin' && has('ms-vscode.cpptools') && onPath('gdb')) return 'gdb';
  if (has('vadimcn.vscode-lldb')) return 'lldb';
  if (has('ms-vscode.cpptools')) return process.platform === 'darwin' ? 'lldb-cpptools' : 'gdb';
  return undefined;
}

/** The native debugger's own launch configuration for the built program. */
function nativeConfiguration(context, config, exe, project) {
  const common = {
    name: config.name,
    request: 'launch',
    program: exe,
    args: config.args ?? [],
    cwd: config.cwd ?? project,
    stopAtEntry: config.stopAtEntry ?? false,
  };
  switch (chooseDebugger(exe, config.debugger)) {
    case 'vsdbg':
      return {
        ...common,
        type: 'cppvsdbg',
        console: 'integratedTerminal',
        visualizerFile: path.join(context.extensionPath, 'dazzle.natvis'),
      };
    case 'gdb':
      return {
        ...common,
        type: 'cppdbg',
        MIMode: 'gdb',
        miDebuggerPath: config.miDebuggerPath ?? onPath('gdb'),
        externalConsole: false,
        setupCommands: [{ text: '-enable-pretty-printing', ignoreFailures: true }],
      };
    case 'lldb-cpptools':
      return { ...common, type: 'cppdbg', MIMode: 'lldb', externalConsole: false };
    case 'lldb':
      return { name: common.name, type: 'lldb', request: 'launch', program: exe, args: common.args, cwd: common.cwd };
    default:
      return undefined;
  }
}

/** Resolves a `dazzle` launch configuration by building and then starting a native debugger. */
function debugProvider(context) {
  return {
    resolveDebugConfiguration(_folder, config) {
      // F5 with no launch.json: debug the active file's project.
      if (!config.type && !config.request && !config.name) {
        const document = vscode.window.activeTextEditor?.document;
        if (document?.languageId !== 'dazzle') return undefined;
        return { type: 'dazzle', request: 'launch', name: 'Debug Dazzle project' };
      }
      return config;
    },

    async resolveDebugConfigurationWithSubstitutedVariables(folder, config) {
      const project = projectFor(config.project);
      if (!project) {
        vscode.window.showErrorMessage(
          'Dazzle: no project to debug — open a file inside one, or set "project" to a directory holding dazzle.toml.',
        );
        return undefined;
      }
      await vscode.workspace.saveAll(false);
      let exe;
      try {
        exe = await buildForDebugging(project);
      } catch (e) {
        const choice = await vscode.window.showErrorMessage(`Dazzle: ${e.message}.`, 'Show Output');
        if (choice) output.show(true);
        return undefined;
      }
      const native = nativeConfiguration(context, config, exe, project);
      if (!native) {
        vscode.window.showErrorMessage(
          'Dazzle: debugging needs the C/C++ extension (ms-vscode.cpptools) or CodeLLDB (vadimcn.vscode-lldb).',
        );
        return undefined;
      }
      output.appendLine(`debugging ${exe} with ${native.type}${native.MIMode ? ` (${native.MIMode})` : ''}`);
      // Handing over rather than returning the other type keeps this working on every
      // editor version; `undefined` ends this resolution without an error.
      await vscode.debug.startDebugging(folder, native);
      return undefined;
    },
  };
}

/** The Debug Project button and command: the active file's project, no launch.json needed. */
async function debugProject() {
  const document = vscode.window.activeTextEditor?.document;
  const project = projectFor(undefined);
  if (!project) {
    vscode.window.showErrorMessage('Dazzle: this file is not part of a project (there is no dazzle.toml above it).');
    return;
  }
  const folder = document ? vscode.workspace.getWorkspaceFolder(document.uri) : undefined;
  await vscode.debug.startDebugging(folder, {
    type: 'dazzle',
    request: 'launch',
    name: `Debug ${path.basename(project)}`,
    project,
  });
}

function activate(context) {
  output = vscode.window.createOutputChannel('Dazzle');
  context.subscriptions.push(
    output,
    vscode.commands.registerCommand('dazzle.run', () => runProject('run')),
    vscode.commands.registerCommand('dazzle.debug', debugProject),
    vscode.debug.registerDebugConfigurationProvider('dazzle', debugProvider(context)),
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
