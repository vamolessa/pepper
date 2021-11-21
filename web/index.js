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
    var term = new Terminal({
        cols: TERMINAL_WIDTH,
        rows: TERMINAL_HEIGHT,
        //rendererType: "dom",
        rendererType: "canvas",
        allowTransparency: false,
        bellStyle: "none",
        convertEol: false,
        screenReaderMode: false,
        scrollback: 0,
    });
    term.open(STATE.terminalElement);
    term.onKey(function(event) {
        const key = event.domEvent.key;
        const ctrl = event.domEvent.ctrlKey;
        const alt = event.domEvent.altKey;

        const displayBytes = pepper_on_event(STATE.pepperApplication, key, ctrl, alt);
        term.writeUtf8(displayBytes);
    });

    const displayBytes = pepper_init(STATE.pepperApplication, term.cols, term.rows);
    term.writeUtf8(displayBytes);
}
