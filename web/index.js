import init, {pepper_new_application, pepper_init, pepper_on_event} from "./pkg/pepper_web.js";

const TERMINAL_WIDTH = 130;
const TERMINAL_HEIGHT = 50;

const STATE = {
    terminalElement: null,
    pepperApplication: null,
};

init().then(() => {
    STATE.pepperApplication = pepper_new_application();
    if (STATE.terminalElement != null) {
        main();
    }
});

window.onload = function() {
    STATE.terminalElement = document.getElementById("terminal");
    if (STATE.pepperApplication != null) {
        main();
    }
}

function main() {
    const terminal = new Terminal({
        cols: TERMINAL_WIDTH,
        rows: TERMINAL_HEIGHT,
        rendererType: "canvas",
        allowTransparency: false,
        bellStyle: "none",
        convertEol: false,
        windowsMode: true,
        screenReaderMode: false,
        scrollback: 0,
        experimentalCharAtlas: "dynamic",
    });

    terminal.open(STATE.terminalElement);

    const webglAddon = new WebglAddon.WebglAddon();
    webglAddon.onContextLoss(e => {
        webglAddon.dispose();
    });
    terminal.loadAddon(webglAddon);

    terminal.onKey(function(event) {
        const key = event.domEvent.key;
        const ctrl = event.domEvent.ctrlKey;
        const alt = event.domEvent.altKey;

        const displayBytes = pepper_on_event(STATE.pepperApplication, key, ctrl, alt);
        terminal.write(displayBytes);
    });

    const displayBytes = pepper_init(STATE.pepperApplication, terminal.cols, terminal.rows);
    terminal.write(displayBytes);
}
