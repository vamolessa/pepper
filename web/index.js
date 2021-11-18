import init, {new_application} from "./pkg/pepper_web.js";

const STATE = {
    terminalElement: null,
    application: null,
};

init().then(() => {
    STATE.application = new_application("helloer");
    if (STATE.terminalElement != null) {
        main();
    }
});

window.onload = function() {
    STATE.terminalElement = document.getElementById("terminal");
    if (STATE.application != null) {
        main();
    }
}

function main() {
    say_hello(STATE.application);
    var term = new Terminal({
        cols: 130,
        rows: 50,
    });
    term.open(STATE.terminalElement);
    term.onKey(function(key) {
        console.log(key);
    });
    term.write("Hello from \x1B[1;3;31mxterm.js\x1B[0m $");
}
