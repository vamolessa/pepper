const TERMINAL_WIDTH = 130;
const TERMINAL_HEIGHT = 50;

const STATE = {
    pepper_wasm_instance: null,
    pepper_application: null,
    terminal_element: null,
};

function u8_array_from_raw_parts(ptr, len) {
    const buffer = STATE.pepper_wasm_instance.exports.memory.buffer;
    return new Uint8Array(buffer, ptr, len);
}

const utf8_decoder = new TextDecoder("utf-8", { ignoreBOM: true, fatal: true });
function console_error(message_ptr, message_len) {
    const utf8_bytes = u8_array_from_raw_parts(message_ptr, message_len);
    const message = utf8_decoder.decode(utf8_bytes);
    console.error(message);
}

function output_to_terminal(terminal) {
    const output_ptr = STATE.pepper_wasm_instance.exports.pepper_output_ptr(STATE.pepper_application);
    const output_len = STATE.pepper_wasm_instance.exports.pepper_output_len(STATE.pepper_application);
    const output = u8_array_from_raw_parts(output_ptr, output_len);
    terminal.write(output);
}

function parse_key(name) {
    const key = {
        kind: 0,
        value: 0,
    };

    if (name == "") {
        key.kind = 0;
    } else if (name == "Backspace") {
        key.kind = 2;
    } else if (name == "ArrowLeft" || name == "Left") {
        key.kind = 3;
    } else if (name == "ArrowRight" || name == "Right") {
        key.kind = 4;
    } else if (name == "ArrowUp" || name == "Up") {
        key.kind = 5;
    } else if (name == "ArrowDown" || name == "Down") {
        key.kind = 6;
    } else if (name == "Home") {
        key.kind = 7;
    } else if (name == "End") {
        key.kind = 8;
    } else if (name == "PageUp") {
        key.kind = 9;
    } else if (name == "PageDown") {
        key.kind = 10;
    } else if (name == "Delete") {
        key.kind = 11;
    } else if (name == "F1") {
        key.kind = 12;
        key.value = 1;
    } else if (name == "F2") {
        key.kind = 12;
        key.value = 2;
    } else if (name == "F3") {
        key.kind = 12;
        key.value = 3;
    } else if (name == "F4") {
        key.kind = 12;
        key.value = 4;
    } else if (name == "F5") {
        key.kind = 12;
        key.value = 5;
    } else if (name == "F6") {
        key.kind = 12;
        key.value = 6;
    } else if (name == "F7") {
        key.kind = 12;
        key.value = 7;
    } else if (name == "F8") {
        key.kind = 12;
        key.value = 8;
    } else if (name == "F9") {
        key.kind = 12;
        key.value = 9;
    } else if (name == "F10") {
        key.kind = 12;
        key.value = 10;
    } else if (name == "F11") {
        key.kind = 12;
        key.value = 11;
    } else if (name == "F12") {
        key.kind = 12;
        key.value = 12;
    } else if (name == "Escape" || name == "Esc") {
        key.kind = 13;
    } else if (name == "Enter") {
        key.kind = 1;
        key.value = "\n".codePointAt(0);
    } else if (name == "Tab") {
        key.kind = 1;
        key.value = "\t".codePointAt(0);
    } else {
        key.kind = 1;
        key.value = name.codePointAt(0);
    }

    return key;
}

const wasm_import_object = {
    env: {
        console_error: console_error,
    }
};
async function load_wasm() {
    const response = await fetch("./pepper_web.wasm");
    const instance = await WebAssembly.instantiateStreaming(response, wasm_import_object);
    return instance.instance;
}
const wasm_instance_promise = load_wasm();

window.onload = async function() {
    STATE.pepper_wasm_instance = await wasm_instance_promise;
    STATE.terminal_element = document.getElementById("terminal");

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

    terminal.open(STATE.terminal_element);

    const webgl_addon = new WebglAddon.WebglAddon();
    webgl_addon.onContextLoss(e => webgl_addon.dispose());
    terminal.loadAddon(webgl_addon);

    terminal.onKey(function(event) {
        const key = parse_key(event.domEvent.key);
        const ctrl = event.domEvent.ctrlKey;
        const alt = event.domEvent.altKey;

        STATE.pepper_wasm_instance.exports.pepper_on_event(STATE.pepper_application, key.kind, key.value, ctrl, alt);
        output_to_terminal(terminal);

        event.domEvent.preventDefault();
    });

    STATE.pepper_application = STATE.pepper_wasm_instance.exports.pepper_init(terminal.cols, terminal.rows);
    output_to_terminal(terminal);
}
