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
            commands::rebase_plan,
            commands::run_rebase,
            commands::read_conflict,
            commands::resolve_region,
            commands::conflict_file_op,
            commands::operation,
            commands::ignore_sources,
            commands::why_ignored,
            commands::add_ignore,
            commands::remove_ignore,
            commands::read_text_file,
            commands::write_ignore_file,
            commands::clone_defaults,
            commands::clone_name,
            commands::clone_repo,
            commands::tags,
            commands::search_log,
            commands::init_repo,
            commands::init_warning,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Gitty");
}
