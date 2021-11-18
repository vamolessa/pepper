import init, {greet} from "./pkg/pepper_web.js";

init().then(() => {
    greet("WebAssembly")
});

window.onload = function() {
    var term = new Terminal();
    term.open(document.getElementById("terminal"));
    term.write("Hello from \x1B[1;3;31mxterm.js\x1B[0m $");
}
