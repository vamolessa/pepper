pub struct CommandCollection {
    //
}

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
*/
