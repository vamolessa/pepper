use crate::{client::ClientManager, editor::Editor};

pub enum CommandOperation {
    Quit,
    QuitAll,
}

struct CommandContext<'a> {
    editor: &'a mut Editor,
    clients: &'a mut ClientManager,
    client_index: usize,
    args: &'a str,
}

struct BuiltinCommand {
    name: &'static str,
    aliases: &'static [&'static str],
    help: &'static str,
    func: fn(CommandContext) -> Option<CommandOperation>,
}

pub struct CommandEngine {
    builtin_commands: Vec<BuiltinCommand>,
    executing_command: String,
}

impl CommandEngine {
    pub fn register_builtin(&mut self, command: BuiltinCommand) {
        self.builtin_commands.push(command);
    }

    pub fn eval(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_index: usize,
    ) -> Option<CommandOperation> {
        None
    }
}

// ------

fn eval(
    editor: &mut Editor,
    clients: &mut ClientManager,
    client_index: usize,
) -> Option<CommandOperation> {
    let command = "";
    let force = false;
    match command {
        "client-quit" | "q" => {
            //
        }
        _ => println!("no such command"),
    }

    None
}

fn client_quit2(commands: &mut CommandEngine) {
    commands.register_builtin(BuiltinCommand {
        name: "",
        aliases: &[],
        help: "",
        func: |ctx| None,
    });
}

//#[command("quits this client"), alias("q")]
fn client_quit(ctx: CommandContext) -> Option<CommandOperation> {
    None
}

fn buffer_open(ctx: CommandContext) -> Option<CommandOperation> {
    None
}

// ------

// picker-entries-from-process-lines "ls" | picker-pick

// $client-index
// $buffer-index
// $buffer-path
// $buffer-view-index

// map-normal k ghla
// map-read-line <c-k> <up>
// buffer-open some/path
// buffer-open "path/with spaces"
// command run-shell {
//  print (process-pipe sh -c (read-line '>'))
// }
//
// process-lines ls {
//  print $0
// }
//
// command find-file {
//   picker-reset
//   read-line-prompt "open(searching...):"
//   process-spawn fd -tf --path-separator / . {
//     read-line-prompt "open:"
//     for-each-line $0 {
//       picker-add-entry $0
//     }
//   }
// }

/*
function find_file()
    picker_reset()
    read_line_prompt("open(searching...):")

    local picked = false
    process_spawn("fd", {"-tf", "--path-separator", "/", "."}, nil, function(output)
        -- this callback is called whenever there's new output from the spawned process
        -- and once more at the end with 'output = nil' to indicate that the process finished
        if picked or output == nil then
            read_line_prompt("open:")
            return
        end
        -- iterate over 'output' lines
        for file in string.gmatch(output, "[^\r\n]+") do
            picker_entry(file)
        end
    end)

    picker_pick(function(file)
        picked = true
        -- if a file was picked, open it
        if file then
            buffer_open(file)
        end
    end)
end

function ripgrep()
    read_line_prompt("rg:")
    -- first a search pattern is read from the user
    read_line_read(function(search_pattern)
        -- early return if action was canceled
        if search_pattern == nil then
            return
        end

        picker_reset()
        read_line_prompt("jump(searching...):")

        local args = {"--line-number"}
        local buffer_path = buffer_path()
        -- maybe restrict searched files based on their extension
        if buffer_path ~= nil then
            local extension = string.match(buffer_path, "[^%.]%.(%w+)$")
            if extension ~= nil then -- only search files with the same extension as current buffer
                args[#args + 1] = "--type-add"
                args[#args + 1] = "t:*." .. extension
                args[#args + 1] = "-tt"
            end
        end
        args[#args + 1] = search_pattern

        local picked = false
        process_spawn("rg", args, nil, function(output)
            -- this callback is called whenever there's new output from the spawned process
            -- and once more at the end with 'output = nil' to indicate that the process finished
            if picked or output == nil then
                read_line_prompt("jump:")
                return
            end
            -- iterate over 'output' lines
            for match in string.gmatch(output, "[^\r\n]+") do
                local file, line, text = string.match(match, "([^:]+):([^:]+):%s*(.*)")
                picker_entry(file .. ":" .. line, text)
            end
        end)

        picker_pick(function(file_and_line)
            picked = true
            -- early return if no file was picked
            if file_and_line == nil then
                return
            end
            local file, line = string.match(file_and_line, "([^:]+):([^:]+)")
            buffer_open(file, line)
        end)
    end)
end
*/
