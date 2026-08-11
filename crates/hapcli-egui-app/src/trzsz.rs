//! Trzsz 文件传输 worker。
//!
//! 与 GPUI 终端共用同一套协议栈：传输句柄由内核会话创建，
//! 本模块负责在后台线程驱动协议循环、读写本地文件并回报进度事件。

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
};

use hapcli_terminal::{TrzszTransferDirection, TrzszTransferPolicy, TrzszTransferSelection};
use hapcli_trzsz::{
    TRZSZ_API_VERSION, TextProgressBar, TrzszDownloadOpenDto, TrzszError, TrzszFileReader,
    TrzszFileWriter, TrzszSaveParam, TrzszState, TrzszTransfer, TrzszUploadEntryDto, download,
    upload,
};
use serde_json::Value;

/// 内核 `TrzszTransferPrompt` 事件的镜像（便于本模块独立测试）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrzszPromptRequest {
    pub direction: TrzszTransferDirection,
    pub selection: TrzszTransferSelection,
    pub remote_is_windows: bool,
}

pub enum TrzszPromptSelection {
    Upload(Vec<String>),
    DownloadRoot(String),
    Cancelled,
}

pub enum TrzszWorkerEvent {
    TerminalOutput(Vec<u8>),
    Completed,
    Cancelled,
    Failed {
        code: String,
        detail: Option<String>,
        message: String,
    },
}

/// 在后台线程启动一次传输，返回事件接收端。
pub fn spawn_trzsz_worker(
    transfer: TrzszTransfer,
    request: TrzszPromptRequest,
    selection: TrzszPromptSelection,
    state: Arc<TrzszState>,
    owner_id: String,
    policy: TrzszTransferPolicy,
    terminal_columns: usize,
) -> Receiver<TrzszWorkerEvent> {
    let (event_tx, event_rx) = channel();
    std::thread::spawn(move || {
        let _ = run_trzsz_worker(
            transfer,
            request,
            selection,
            state,
            owner_id,
            policy,
            event_tx,
            terminal_columns,
        );
    });
    event_rx
}

fn run_trzsz_worker(
    mut transfer: TrzszTransfer,
    request: TrzszPromptRequest,
    selection: TrzszPromptSelection,
    state: Arc<TrzszState>,
    owner_id: String,
    policy: TrzszTransferPolicy,
    event_tx: Sender<TrzszWorkerEvent>,
    terminal_columns: usize,
) -> Result<(), TrzszError> {
    let result = match request.direction {
        TrzszTransferDirection::Upload => {
            run_upload(&mut transfer, &request, &selection, &state, &owner_id, &policy, &event_tx, terminal_columns)
        }
        TrzszTransferDirection::Download => run_download(
            &mut transfer,
            &request,
            &selection,
            &state,
            &owner_id,
            &policy,
            &event_tx,
            terminal_columns,
        ),
    };

    if let Err(error) = &result
        && !is_cancelled_transfer(error)
    {
        let _ = transfer.client_error(error);
    }
    let _ = state.cleanup_owner(&owner_id);

    let event = match result.as_ref() {
        Ok(()) => TrzszWorkerEvent::Completed,
        Err(error) if is_cancelled_transfer(error) => TrzszWorkerEvent::Cancelled,
        Err(error) => TrzszWorkerEvent::Failed {
            code: error.code().as_str().to_string(),
            detail: error.detail(),
            message: error.to_string(),
        },
    };
    let _ = event_tx.send(event);
    result
}

fn run_upload(
    transfer: &mut TrzszTransfer,
    request: &TrzszPromptRequest,
    selection: &TrzszPromptSelection,
    state: &Arc<TrzszState>,
    owner_id: &str,
    policy: &TrzszTransferPolicy,
    event_tx: &Sender<TrzszWorkerEvent>,
    terminal_columns: usize,
) -> Result<(), TrzszError> {
    let paths = match selection {
        TrzszPromptSelection::Upload(paths) if !paths.is_empty() => paths.clone(),
        _ => {
            transfer.send_action(false, request.remote_is_windows)?;
            return Err(TrzszError::InvalidState("Stopped".to_string()));
        }
    };

    let directory = request.selection == TrzszTransferSelection::Directory;
    if directory && !policy.allow_directory {
        return Err(TrzszError::DirectoryNotAllowed(
            "terminal settings".to_string(),
        ));
    }

    let readers = build_upload_readers(state.clone(), owner_id, paths, policy)?;
    if readers.is_empty() {
        transfer.send_action(false, request.remote_is_windows)?;
        return Err(TrzszError::InvalidState("Stopped".to_string()));
    }

    transfer.send_action(true, request.remote_is_windows)?;
    let config = transfer.recv_config()?;
    if config
        .get("overwrite")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        check_duplicate_names(&readers)?;
    }

    let mut progress = progress_from_config(terminal_columns, event_tx, &config);
    let send_result = transfer.send_files(readers, progress.as_mut());
    if let Some(progress) = progress.as_mut() {
        progress.show_cursor();
    }
    let remote_names = send_result?;
    transfer.client_exit(&format_saved_files(&remote_names, ""))
}

fn run_download(
    transfer: &mut TrzszTransfer,
    request: &TrzszPromptRequest,
    selection: &TrzszPromptSelection,
    state: &Arc<TrzszState>,
    owner_id: &str,
    policy: &TrzszTransferPolicy,
    event_tx: &Sender<TrzszWorkerEvent>,
    terminal_columns: usize,
) -> Result<(), TrzszError> {
    let root_path = match selection {
        TrzszPromptSelection::DownloadRoot(root_path) => root_path.clone(),
        _ => {
            transfer.send_action(false, request.remote_is_windows)?;
            return Err(TrzszError::InvalidState("Stopped".to_string()));
        }
    };

    let prepared = download::prepare_download_root(state, owner_id, TRZSZ_API_VERSION, root_path)?;
    transfer.send_action(true, request.remote_is_windows)?;
    let config = transfer.recv_config()?;
    let directory = config
        .get("directory")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if directory && !policy.allow_directory {
        return Err(TrzszError::DirectoryNotAllowed(
            "terminal settings".to_string(),
        ));
    }

    let display_name = base_name(&prepared.root_path);
    let mut save_root = NativeSaveRoot {
        root_path: prepared.root_path,
        display_name,
        maps: HashMap::new(),
    };
    let mut constraints = DownloadConstraintTracker::new(policy.clone());
    let state = state.clone();
    let owner_id = owner_id.to_string();
    let mut progress = progress_from_config(terminal_columns, event_tx, &config);
    let recv_result = transfer.recv_files(
        &TrzszSaveParam {
            root_path: save_root.root_path.clone(),
            display_name: save_root.display_name.clone(),
        },
        |save_param, file_name, directory, overwrite| {
            open_save_file(
                state.clone(),
                owner_id.clone(),
                &mut save_root,
                save_param,
                file_name,
                directory,
                overwrite,
                &mut constraints,
            )
        },
        progress.as_mut(),
    );
    if let Some(progress) = progress.as_mut() {
        progress.show_cursor();
    }
    let local_names = recv_result?;
    transfer.client_exit(&format_saved_files(&local_names, &save_root.root_path))
}

fn build_upload_readers(
    state: Arc<TrzszState>,
    owner_id: &str,
    paths: Vec<String>,
    policy: &TrzszTransferPolicy,
) -> Result<Vec<Box<dyn TrzszFileReader>>, TrzszError> {
    let entries = upload::build_upload_entries(
        &state,
        owner_id,
        TRZSZ_API_VERSION,
        paths,
        policy.allow_directory,
    )?;
    if !policy.allow_directory
        && entries
            .iter()
            .any(|entry| entry.is_dir || entry.rel_path.len() > 1)
    {
        return Err(TrzszError::DirectoryNotAllowed(
            "terminal settings".to_string(),
        ));
    }

    let file_count = entries.iter().filter(|entry| !entry.is_dir).count();
    if file_count > policy.max_file_count {
        return Err(TrzszError::MaxFileCountExceeded {
            selected: file_count,
            max: policy.max_file_count,
        });
    }

    let total_bytes = entries
        .iter()
        .filter(|entry| !entry.is_dir)
        .map(|entry| entry.size)
        .sum::<u64>();
    if total_bytes > policy.max_total_bytes {
        return Err(TrzszError::MaxTotalBytesExceeded {
            selected: total_bytes,
            max: policy.max_total_bytes,
        });
    }

    Ok(entries
        .into_iter()
        .map(|entry| {
            Box::new(NativeUploadReader::new(
                state.clone(),
                owner_id.to_string(),
                entry,
            )) as Box<dyn TrzszFileReader>
        })
        .collect())
}

struct NativeUploadReader {
    state: Arc<TrzszState>,
    owner_id: String,
    entry: TrzszUploadEntryDto,
    handle_id: Option<String>,
    offset: u64,
    closed: bool,
}

impl NativeUploadReader {
    fn new(state: Arc<TrzszState>, owner_id: String, entry: TrzszUploadEntryDto) -> Self {
        Self {
            state,
            owner_id,
            entry,
            handle_id: None,
            offset: 0,
            closed: false,
        }
    }

    fn ensure_handle(&mut self) -> Result<String, TrzszError> {
        if let Some(handle_id) = &self.handle_id {
            return Ok(handle_id.clone());
        }
        let handle = upload::open_upload_file(
            &self.state,
            &self.owner_id,
            TRZSZ_API_VERSION,
            self.entry.path.clone(),
        )?;
        self.handle_id = Some(handle.handle_id.clone());
        Ok(handle.handle_id)
    }
}

impl TrzszFileReader for NativeUploadReader {
    fn close_file(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        if let Some(handle_id) = self.handle_id.take() {
            let _ = upload::close_upload_file(
                &self.state,
                &self.owner_id,
                TRZSZ_API_VERSION,
                &handle_id,
            );
        }
    }

    fn path_id(&self) -> u64 {
        self.entry.path_id
    }

    fn rel_path(&self) -> &[String] {
        &self.entry.rel_path
    }

    fn is_dir(&self) -> bool {
        self.entry.is_dir
    }

    fn size(&self) -> u64 {
        self.entry.size
    }

    fn read_file(&mut self, max_len: usize) -> Result<Vec<u8>, TrzszError> {
        if self.closed || self.entry.is_dir {
            return Ok(Vec::new());
        }
        let handle_id = self.ensure_handle()?;
        let data = upload::read_upload_chunk(
            &self.state,
            &self.owner_id,
            TRZSZ_API_VERSION,
            &handle_id,
            self.offset,
            max_len,
        )?;
        self.offset = self.offset.saturating_add(data.len() as u64);
        Ok(data)
    }
}

impl Drop for NativeUploadReader {
    fn drop(&mut self) {
        self.close_file();
    }
}

fn progress_from_config(
    terminal_columns: usize,
    event_tx: &Sender<TrzszWorkerEvent>,
    config: &Value,
) -> Option<TextProgressBar> {
    if config
        .get("quiet")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }

    let tmux_pane_columns = config
        .get("tmux_pane_width")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok());
    let event_tx = event_tx.clone();
    let writer = Arc::new(move |output: String| {
        let _ = event_tx.send(TrzszWorkerEvent::TerminalOutput(output.into_bytes()));
    });
    let mut progress =
        TextProgressBar::new_with_writer(terminal_columns.max(1), tmux_pane_columns, writer);
    progress.hide_cursor();
    Some(progress)
}

struct NativeSaveRoot {
    root_path: String,
    display_name: String,
    maps: HashMap<u64, String>,
}

struct DownloadConstraintTracker {
    policy: TrzszTransferPolicy,
    file_count: usize,
    total_bytes: Arc<Mutex<u64>>,
}

impl DownloadConstraintTracker {
    fn new(policy: TrzszTransferPolicy) -> Self {
        Self {
            policy,
            file_count: 0,
            total_bytes: Arc::new(Mutex::new(0)),
        }
    }

    fn ensure_directory_allowed(&self) -> Result<(), TrzszError> {
        if self.policy.allow_directory {
            Ok(())
        } else {
            Err(TrzszError::DirectoryNotAllowed(
                "terminal settings".to_string(),
            ))
        }
    }

    fn assert_can_add_file(&self) -> Result<(), TrzszError> {
        if self.file_count + 1 > self.policy.max_file_count {
            Err(TrzszError::MaxFileCountExceeded {
                selected: self.file_count + 1,
                max: self.policy.max_file_count,
            })
        } else {
            Ok(())
        }
    }

    fn commit_file(&mut self) {
        self.file_count = self.file_count.saturating_add(1);
    }

    fn byte_counter(&self) -> Arc<Mutex<u64>> {
        self.total_bytes.clone()
    }
}

fn open_save_file(
    state: Arc<TrzszState>,
    owner_id: String,
    save_root: &mut NativeSaveRoot,
    save_param: &TrzszSaveParam,
    file_name: &str,
    directory: bool,
    overwrite: bool,
    constraints: &mut DownloadConstraintTracker,
) -> Result<Box<dyn TrzszFileWriter>, TrzszError> {
    if !directory {
        return open_flat_save_file(
            state,
            owner_id,
            save_param,
            file_name,
            overwrite,
            constraints,
        );
    }

    let entry = parse_directory_entry(file_name)?;
    open_directory_save_entry(state, owner_id, save_root, entry, overwrite, constraints)
}

fn open_flat_save_file(
    state: Arc<TrzszState>,
    owner_id: String,
    save_param: &TrzszSaveParam,
    file_name: &str,
    overwrite: bool,
    constraints: &mut DownloadConstraintTracker,
) -> Result<Box<dyn TrzszFileWriter>, TrzszError> {
    let mut last_error = None;
    for attempt in 0..1000 {
        let candidate = if overwrite {
            file_name.to_string()
        } else {
            next_collision_name(file_name, attempt)
        };
        match try_open_download_file(
            state.clone(),
            owner_id.clone(),
            save_param.root_path.clone(),
            candidate.clone(),
            file_name.to_string(),
            candidate.clone(),
            Vec::new(),
            overwrite,
            constraints,
        ) {
            Ok(writer) => return Ok(Box::new(writer)),
            Err(error) if !overwrite && is_retryable_collision(&error) => {
                last_error = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| TrzszError::InvalidPath(file_name.to_string())))
}

fn open_directory_save_entry(
    state: Arc<TrzszState>,
    owner_id: String,
    save_root: &mut NativeSaveRoot,
    entry: TrzszDirectoryEntry,
    overwrite: bool,
    constraints: &mut DownloadConstraintTracker,
) -> Result<Box<dyn TrzszFileWriter>, TrzszError> {
    let existing_local_name = if overwrite {
        Some(entry.path_name[0].clone())
    } else {
        save_root.maps.get(&entry.path_id).cloned()
    };
    let rest_path = entry.path_name[1..].to_vec();

    if let Some(local_root) = existing_local_name {
        return try_open_with_root(
            state,
            owner_id,
            save_root,
            &entry,
            rest_path,
            local_root,
            false,
            overwrite,
            constraints,
        );
    }

    let mut last_error = None;
    for attempt in 0..1000 {
        let local_root = if overwrite {
            entry.path_name[0].clone()
        } else {
            next_collision_name(&entry.path_name[0], attempt)
        };
        match try_open_with_root(
            state.clone(),
            owner_id.clone(),
            save_root,
            &entry,
            rest_path.clone(),
            local_root.clone(),
            true,
            overwrite,
            constraints,
        ) {
            Ok(writer) => {
                save_root.maps.insert(entry.path_id, local_root);
                return Ok(writer);
            }
            Err(error) if !overwrite && is_retryable_collision(&error) => last_error = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| TrzszError::InvalidPath(entry.path_name.join("/"))))
}

fn try_open_with_root(
    state: Arc<TrzszState>,
    owner_id: String,
    save_root: &NativeSaveRoot,
    entry: &TrzszDirectoryEntry,
    rest_path: Vec<String>,
    local_root: String,
    claim_top_level: bool,
    overwrite: bool,
    constraints: &mut DownloadConstraintTracker,
) -> Result<Box<dyn TrzszFileWriter>, TrzszError> {
    let mut cleanup_directories = Vec::new();
    let result = (|| {
        if entry.is_dir || !rest_path.is_empty() {
            constraints.ensure_directory_allowed()?;
        }

        if claim_top_level && (entry.is_dir || !rest_path.is_empty()) {
            ensure_download_directory(
                &state,
                &owner_id,
                &save_root.root_path,
                &local_root,
                &mut cleanup_directories,
                !overwrite,
            )?;
        }

        let relative_path = join_path(std::iter::once(local_root.clone()).chain(rest_path.clone()));
        if entry.is_dir {
            for index in 0..rest_path.len() {
                let path = join_path(
                    std::iter::once(local_root.clone()).chain(rest_path[..=index].iter().cloned()),
                );
                ensure_download_directory(
                    &state,
                    &owner_id,
                    &save_root.root_path,
                    &path,
                    &mut cleanup_directories,
                    false,
                )?;
            }
            return Ok(Box::new(NativeDirectoryWriter {
                state: state.clone(),
                owner_id: owner_id.clone(),
                root_path: save_root.root_path.clone(),
                cleanup_directories: cleanup_directories.clone(),
                file_name: entry
                    .path_name
                    .last()
                    .cloned()
                    .unwrap_or_else(|| local_root.clone()),
                local_name: local_root,
            }) as Box<dyn TrzszFileWriter>);
        }

        constraints.assert_can_add_file()?;
        for index in 0..rest_path.len().saturating_sub(1) {
            let path = join_path(
                std::iter::once(local_root.clone()).chain(rest_path[..=index].iter().cloned()),
            );
            ensure_download_directory(
                &state,
                &owner_id,
                &save_root.root_path,
                &path,
                &mut cleanup_directories,
                false,
            )?;
        }

        let writer = try_open_download_file(
            state.clone(),
            owner_id.clone(),
            save_root.root_path.clone(),
            relative_path,
            entry
                .path_name
                .last()
                .cloned()
                .unwrap_or_else(|| local_root.clone()),
            local_root,
            cleanup_directories.clone(),
            overwrite,
            constraints,
        )?;
        Ok(Box::new(writer) as Box<dyn TrzszFileWriter>)
    })();

    if result.is_err() {
        for directory_path in cleanup_directories.iter().rev() {
            let _ = download::remove_download_directory(
                &state,
                &owner_id,
                TRZSZ_API_VERSION,
                save_root.root_path.clone(),
                directory_path.clone(),
            );
        }
    }
    result
}

fn try_open_download_file(
    state: Arc<TrzszState>,
    owner_id: String,
    root_path: String,
    relative_path: String,
    file_name: String,
    local_name: String,
    cleanup_directories: Vec<String>,
    overwrite: bool,
    constraints: &mut DownloadConstraintTracker,
) -> Result<NativeDownloadFileWriter, TrzszError> {
    let dto = download::open_save_file(
        &state,
        &owner_id,
        TRZSZ_API_VERSION,
        root_path.clone(),
        relative_path.clone(),
        false,
        overwrite,
    )?;
    constraints.commit_file();
    Ok(NativeDownloadFileWriter::new(
        state,
        owner_id,
        dto,
        root_path,
        relative_path,
        file_name,
        local_name,
        cleanup_directories,
        constraints,
    ))
}

fn ensure_download_directory(
    state: &TrzszState,
    owner_id: &str,
    root_path: &str,
    directory_path: &str,
    cleanup_directories: &mut Vec<String>,
    must_create: bool,
) -> Result<(), TrzszError> {
    let dto = download::create_download_directory(
        state,
        owner_id,
        TRZSZ_API_VERSION,
        root_path.to_string(),
        directory_path.to_string(),
        must_create,
    )?;
    if dto.created {
        cleanup_directories.push(directory_path.to_string());
    }
    Ok(())
}

struct NativeDirectoryWriter {
    state: Arc<TrzszState>,
    owner_id: String,
    root_path: String,
    cleanup_directories: Vec<String>,
    file_name: String,
    local_name: String,
}

impl TrzszFileWriter for NativeDirectoryWriter {
    fn close_file(&mut self) {}

    fn file_name(&self) -> &str {
        &self.file_name
    }

    fn local_name(&self) -> &str {
        &self.local_name
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn write_file(&mut self, _data: &[u8]) -> Result<(), TrzszError> {
        Err(TrzszError::InvalidState(format!(
            "Cannot write data into directory: {}",
            self.file_name
        )))
    }

    fn delete_file(&mut self) -> Result<String, TrzszError> {
        self.abort_file()?;
        Ok(String::new())
    }

    fn commit_file(&mut self) -> Result<(), TrzszError> {
        for directory_path in &self.cleanup_directories {
            download::commit_download_directory(
                &self.state,
                &self.owner_id,
                TRZSZ_API_VERSION,
                self.root_path.clone(),
                directory_path.clone(),
            )?;
        }
        Ok(())
    }

    fn abort_file(&mut self) -> Result<(), TrzszError> {
        for directory_path in self.cleanup_directories.iter().rev() {
            download::remove_download_directory(
                &self.state,
                &self.owner_id,
                TRZSZ_API_VERSION,
                self.root_path.clone(),
                directory_path.clone(),
            )?;
        }
        Ok(())
    }
}

struct NativeDownloadFileWriter {
    state: Arc<TrzszState>,
    owner_id: String,
    writer_id: String,
    root_path: String,
    relative_path: String,
    file_name: String,
    local_name: String,
    cleanup_directories: Vec<String>,
    finished: bool,
    aborted: bool,
    finish_started: bool,
    total_limit: u64,
    total_bytes: Arc<Mutex<u64>>,
}

impl NativeDownloadFileWriter {
    fn new(
        state: Arc<TrzszState>,
        owner_id: String,
        dto: TrzszDownloadOpenDto,
        root_path: String,
        relative_path: String,
        file_name: String,
        local_name: String,
        cleanup_directories: Vec<String>,
        constraints: &DownloadConstraintTracker,
    ) -> Self {
        Self {
            state,
            owner_id,
            writer_id: dto.writer_id,
            root_path,
            relative_path,
            file_name,
            local_name,
            cleanup_directories,
            finished: false,
            aborted: false,
            finish_started: false,
            total_limit: constraints.policy.max_total_bytes,
            total_bytes: constraints.byte_counter(),
        }
    }
}

impl TrzszFileWriter for NativeDownloadFileWriter {
    fn close_file(&mut self) {}

    fn file_name(&self) -> &str {
        &self.file_name
    }

    fn local_name(&self) -> &str {
        &self.local_name
    }

    fn is_dir(&self) -> bool {
        false
    }

    fn write_file(&mut self, data: &[u8]) -> Result<(), TrzszError> {
        if self.finished || self.aborted {
            return Err(TrzszError::InvalidState(format!(
                "Download writer is no longer active: {}",
                self.file_name
            )));
        }
        let mut total_bytes = self
            .total_bytes
            .lock()
            .expect("trzsz download byte counter");
        *total_bytes = total_bytes.saturating_add(data.len() as u64);
        if *total_bytes > self.total_limit {
            return Err(TrzszError::MaxTotalBytesExceeded {
                selected: *total_bytes,
                max: self.total_limit,
            });
        }
        drop(total_bytes);
        download::write_download_chunk(
            &self.state,
            &self.owner_id,
            TRZSZ_API_VERSION,
            &self.writer_id,
            data.to_vec(),
        )
    }

    fn delete_file(&mut self) -> Result<String, TrzszError> {
        if self.finished {
            download::remove_download_file(
                &self.state,
                &self.owner_id,
                TRZSZ_API_VERSION,
                self.root_path.clone(),
                self.relative_path.clone(),
            )?;
        } else if let Err(error) = self.abort_file() {
            if !(self.finish_started && matches!(error, TrzszError::HandleNotFound(_))) {
                return Err(error);
            }
            download::remove_download_file(
                &self.state,
                &self.owner_id,
                TRZSZ_API_VERSION,
                self.root_path.clone(),
                self.relative_path.clone(),
            )?;
        }
        for directory_path in self.cleanup_directories.iter().rev() {
            download::remove_download_directory(
                &self.state,
                &self.owner_id,
                TRZSZ_API_VERSION,
                self.root_path.clone(),
                directory_path.clone(),
            )?;
        }
        Ok(String::new())
    }

    fn commit_file(&mut self) -> Result<(), TrzszError> {
        for directory_path in &self.cleanup_directories {
            download::commit_download_directory(
                &self.state,
                &self.owner_id,
                TRZSZ_API_VERSION,
                self.root_path.clone(),
                directory_path.clone(),
            )?;
        }
        Ok(())
    }

    fn finish_file(&mut self) -> Result<(), TrzszError> {
        if self.finished || self.aborted {
            return Ok(());
        }
        self.finish_started = true;
        download::finish_download_file(
            &self.state,
            &self.owner_id,
            TRZSZ_API_VERSION,
            &self.writer_id,
        )?;
        self.finished = true;
        Ok(())
    }

    fn abort_file(&mut self) -> Result<(), TrzszError> {
        if self.finished || self.aborted {
            return Ok(());
        }
        download::abort_download_file(
            &self.state,
            &self.owner_id,
            TRZSZ_API_VERSION,
            &self.writer_id,
        )?;
        self.aborted = true;
        Ok(())
    }
}

#[derive(Debug)]
struct TrzszDirectoryEntry {
    path_id: u64,
    path_name: Vec<String>,
    is_dir: bool,
}

fn parse_directory_entry(raw: &str) -> Result<TrzszDirectoryEntry, TrzszError> {
    let payload: Value =
        serde_json::from_str(raw).map_err(|error| TrzszError::InvalidPath(error.to_string()))?;
    let path_name = payload
        .get("path_name")
        .and_then(Value::as_array)
        .ok_or_else(|| TrzszError::InvalidPath(format!("Invalid directory entry: {raw}")))?
        .iter()
        .map(|value| value.as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();
    if path_name.is_empty() {
        return Err(TrzszError::InvalidPath(format!(
            "Invalid directory entry: {raw}"
        )));
    }
    Ok(TrzszDirectoryEntry {
        path_id: payload
            .get("path_id")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        path_name,
        is_dir: payload
            .get("is_dir")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn check_duplicate_names(files: &[Box<dyn TrzszFileReader>]) -> Result<(), TrzszError> {
    let mut names = HashSet::new();
    for file in files {
        let path = file.rel_path().join("/");
        if !names.insert(path.clone()) {
            return Err(TrzszError::InvalidState(format!("Duplicate name: {path}")));
        }
    }
    Ok(())
}

fn next_collision_name(base_name: &str, attempt: usize) -> String {
    if attempt == 0 {
        base_name.to_string()
    } else {
        format!("{base_name}.{}", attempt - 1)
    }
}

fn is_retryable_collision(error: &TrzszError) -> bool {
    match error {
        TrzszError::AlreadyExists(_) => true,
        TrzszError::InvalidPath(message) => {
            message.contains("resolves to a directory")
                || message.contains("resolves to a file")
                || message.contains("Target path is a directory")
        }
        _ => false,
    }
}

fn is_cancelled_transfer(error: &TrzszError) -> bool {
    matches!(error, TrzszError::InvalidState(message) if message == "Stopped")
}

fn join_path(parts: impl IntoIterator<Item = String>) -> String {
    parts.into_iter().collect::<Vec<_>>().join("/")
}

fn base_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.trim_end_matches(['/', '\\']).to_string())
}

fn format_saved_files(file_names: &[String], dest_path: &str) -> String {
    let mut message = format!(
        "Saved {} {}",
        file_names.len(),
        if file_names.len() > 1 {
            "files/directories"
        } else {
            "file/directory"
        }
    );
    if !dest_path.is_empty() {
        message.push_str(" to ");
        message.push_str(dest_path);
    }
    let mut lines = vec![message];
    lines.extend(file_names.iter().cloned());
    lines.join("\r\n- ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hapcli_trzsz_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn build_upload_readers_enforces_file_count() {
        let dir = temp_dir("file_count");
        fs::write(dir.join("a.txt"), b"a").unwrap();
        fs::write(dir.join("b.txt"), b"b").unwrap();
        let state = TrzszState::new();
        let mut policy = TrzszTransferPolicy::default();
        policy.max_file_count = 1;
        let result = build_upload_readers(
            state.clone(),
            "test-owner",
            vec![dir.join("a.txt").display().to_string(), dir.join("b.txt").display().to_string()],
            &policy,
        );
        assert!(matches!(result, Err(TrzszError::MaxFileCountExceeded { .. })));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn build_upload_readers_rejects_directory_when_disallowed() {
        let dir = temp_dir("dir_disallowed");
        fs::write(dir.join("a.txt"), b"a").unwrap();
        let state = TrzszState::new();
        let mut policy = TrzszTransferPolicy::default();
        policy.allow_directory = false;
        let result = build_upload_readers(
            state.clone(),
            "test-owner",
            vec![dir.display().to_string()],
            &policy,
        );
        assert!(matches!(
            result,
            Err(TrzszError::DirectoryNotAllowed(_))
        ));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_directory_entry_handles_json() {
        let entry = parse_directory_entry(
            r#"{"path_id":7,"path_name":["docs","a.txt"],"is_dir":false}"#,
        )
        .unwrap();
        assert_eq!(entry.path_id, 7);
        assert_eq!(entry.path_name, vec!["docs", "a.txt"]);
        assert!(!entry.is_dir);
        assert!(parse_directory_entry("not json").is_err());
    }

    #[test]
    fn collision_and_cancel_helpers() {
        assert_eq!(next_collision_name("a.txt", 0), "a.txt");
        assert_eq!(next_collision_name("a.txt", 1), "a.txt.0");
        assert!(is_cancelled_transfer(&TrzszError::InvalidState("Stopped".to_string())));
        assert!(!is_cancelled_transfer(&TrzszError::InvalidState("boom".to_string())));
    }
}
