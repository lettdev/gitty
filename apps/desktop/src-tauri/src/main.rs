#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::initial_repo,
            commands::open_repo,
            commands::start_view,
            commands::repo_list,
            commands::status,
            commands::file_diff,
            commands::commit_file_diff,
            commands::stage,
            commands::unstage,
            commands::stage_all,
            commands::apply_lines,
            commands::discard_file,
            commands::commit,
            commands::last_message,
            commands::undo,
            commands::reword,
            commands::log,
            commands::commit_detail,
            commands::commit_message,
            commands::branches,
            commands::switch_branch,
            commands::create_branch,
            commands::stash_push,
            commands::stash_apply,
            commands::remote_op,
            commands::insight,
            commands::largest_blobs,
            commands::apply_fix,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Gitty");
}
