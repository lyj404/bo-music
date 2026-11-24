//! Command routes for accounts.

macro_rules! accounts_arms {
    ($command:ident [$($arms:tt)*] $rest:tt;
        $events:ident, $sender:ident, $transport:ident,
        $preferences:ident, $queue_store:ident, $persist_enabled:ident,
        $player:ident, $query_generation:ident, $query_text:ident,
        $search_results:ident, $search_loading:ident, $search_task:ident,
        $lyrics_key:ident, $lyrics_generation:ident, $lyrics_task:ident,
        $browser:ident, $qr_task:ident, $qr_generation:ident,
        $qr_interval:ident, $qr_lifetime:ident, $account_list_generation:ident,
        $cancel_account_requests:ident, $cancel_qr:ident, $secrets:ident,
        $account:ident, $auth_busy:ident, $sms_busy:ident,
        $last_sms:ident $(,)?
    ) => {
        collect_command_arms!($command [$($arms)*
            Command::StartQr => {
                if $auth_busy || $sms_busy {
                    continue;
                }
                if $secrets.is_none() {
                    auth_view(&$events, &$account, false, "系统凭据存储不可用");
                    continue;
                }
                $cancel_qr!();
                let _ = $events.send(Event::Qr(QrView::Loading));
                $qr_task = Some(launch_qr(
                    Arc::clone(&$transport),
                    $sender.clone(),
                    $qr_generation,
                    $qr_interval,
                    $qr_lifetime,
                ));
            }
            Command::CancelQr => {
                if !$auth_busy {
                    $cancel_qr!();
                }
            }
            Command::QrUpdate(generation, update)
                if generation == $qr_generation && !$auth_busy =>
            {
                match update {
                    QrUpdate::Ready(url) => {
                        let _ = $events.send(Event::Qr(QrView::Ready(url)));
                    }
                    QrUpdate::Expired => {
                        $qr_task = None;
                        let _ = $events.send(Event::Qr(QrView::Expired));
                    }
                    QrUpdate::Failed => {
                        $qr_task = None;
                        let _ = $events.send(Event::Qr(QrView::Failed));
                    }
                    QrUpdate::Complete(ExchangeOutcome::Credential(credential)) => {
                        $qr_task = None;
                        let Some(backend) = $secrets.clone() else {
                            let _ = $events.send(Event::Qr(QrView::Failed));
                            continue;
                        };
                        if $persist_enabled
                            && let Some($player) = $player.as_ref()
                            && $player.save_account_queue(&$queue_store).is_err()
                        {
                            let _ = $events.send(Event::Qr(QrView::Failed));
                            auth_view(&$events, &$account, false, "当前队列保存失败，请重试");
                            continue;
                        }
                        $cancel_account_requests!();
                        $auth_busy = true;
                        let _ = $events.send(Event::Qr(QrView::Idle));
                        auth_view(&$events, &$account, true, "正在保存登录…");
                        let $sender = $sender.clone();
                        std::thread::spawn(move || {
                            let result =
                                auth::install(credential, &CredentialStore::new(backend)).map(Some);
                            let _ = $sender.send(Command::AuthDone(AuthOperation::Login, result));
                        });
                    }
                    QrUpdate::Complete(_) => {
                        $qr_task = None;
                        let _ = $events.send(Event::Qr(QrView::Failed));
                    }
                }
            }
            Command::QrUpdate(..) => {}
            Command::RequestSms(raw) => {
                let Some(mobile) = normalize_mobile(&raw) else {
                    auth_view(&$events, &$account, $auth_busy, "手机号格式不正确");
                    continue;
                };
                if $auth_busy || $sms_busy {
                    continue;
                }
                if $last_sms.is_some_and(|last| last.elapsed() < Duration::from_secs(60)) {
                    auth_view(
                        &$events,
                        &$account,
                        false,
                        "验证码发送过于频繁，请稍后重试",
                    );
                    continue;
                }
                $cancel_qr!();
                $sms_busy = true;
                $last_sms = Some(std::time::Instant::now());
                auth_view(&$events, &$account, false, "正在发送验证码…");
                let $transport = Arc::clone(&$transport);
                let $sender = $sender.clone();
                tokio::spawn(async move {
                    let result = login::send_sms(&$transport, &mobile)
                        .await
                        .map_err(|_| "验证码发送失败".into());
                    let _ = $sender.send(Command::SmsDone(result));
                });
            }
            Command::SmsDone(result) => {
                $sms_busy = false;
                let message = match result {
                    Ok(SmsOutcome::Sent) => "验证码已发送，请查收短信",
                    Ok(SmsOutcome::Failed { .. }) | Err(_) => "验证码发送失败，请稍后重试",
                };
                auth_view(&$events, &$account, $auth_busy, message);
            }
            Command::LoginPhone(raw, code) => {
                if $auth_busy {
                    continue;
                }
                if $sms_busy {
                    auth_view(&$events, &$account, false, "验证码发送中，请稍候");
                    continue;
                }
                let Some(mobile) = normalize_mobile(&raw) else {
                    auth_view(&$events, &$account, false, "手机号格式不正确");
                    continue;
                };
                let code = code.trim().to_owned();
                if code.is_empty() || code.len() > 20 || !code.bytes().all(|c| c.is_ascii_digit()) {
                    auth_view(&$events, &$account, false, "请输入有效的短信验证码");
                    continue;
                }
                if $secrets.is_none() {
                    auth_view(&$events, &$account, false, "系统凭据存储不可用");
                    continue;
                }
                if $persist_enabled
                    && let Some($player) = $player.as_ref()
                    && $player.save_account_queue(&$queue_store).is_err()
                {
                    auth_view(&$events, &$account, false, "当前队列保存失败，请重试");
                    continue;
                }
                $cancel_qr!();
                $cancel_account_requests!();
                $auth_busy = true;
                auth_view(&$events, &$account, true, "正在登录…");
                let $transport = Arc::clone(&$transport);
                let $sender = $sender.clone();
                tokio::spawn(async move {
                    let result = login::exchange_phone(&$transport, &mobile, &code)
                        .await
                        .map_err(|_| "登录失败，请检查网络".into());
                    let _ = $sender.send(Command::PhoneDone(result));
                });
            }
            Command::PhoneDone(result) => match result {
                Ok(ExchangeOutcome::Credential(credential)) => {
                    let backend = $secrets
                        .clone()
                        .expect("login requires credential backend");
                    let $sender = $sender.clone();
                    std::thread::spawn(move || {
                        let result =
                            auth::install(credential, &CredentialStore::new(backend)).map(Some);
                        let _ = $sender.send(Command::AuthDone(AuthOperation::Login, result));
                    });
                }
                outcome => {
                    $auth_busy = false;
                    let message = match outcome {
                        Ok(ExchangeOutcome::WrongCode) => "验证码不正确",
                        Ok(ExchangeOutcome::IdentityMismatch)
                        | Ok(ExchangeOutcome::InvalidResponse) => "登录响应的身份无法验证",
                        _ => "登录失败，请稍后重试",
                    };
                    auth_view(&$events, &$account, false, message);
                }
            },
            Command::SignOut => {
                if $auth_busy {
                    continue;
                }
                let Some(backend) = $secrets.clone() else {
                    auth_view(&$events, &$account, false, "系统凭据存储不可用");
                    continue;
                };
                if $persist_enabled
                    && let Some($player) = $player.as_ref()
                    && $player.save_account_queue(&$queue_store).is_err()
                {
                    auth_view(&$events, &$account, false, "当前队列保存失败，请重试");
                    continue;
                }
                $cancel_qr!();
                $cancel_account_requests!();
                $auth_busy = true;
                auth_view(&$events, &$account, true, "正在退出登录…");
                let session = $transport.session();
                let $sender = $sender.clone();
                std::thread::spawn(move || {
                    let result =
                        auth::sign_out(&session, &CredentialStore::new(backend)).map(|_| None);
                    let _ = $sender.send(Command::AuthDone(AuthOperation::SignOut, result));
                });
            }
            Command::SwitchAccount(uid) => {
                if $auth_busy || !uid.parse::<i64>().is_ok_and(|id| id > 0) {
                    continue;
                }
                if $account.as_ref().is_some_and(|current| current.uid == uid) {
                    continue;
                }
                let Some(backend) = $secrets.clone() else {
                    auth_view(&$events, &$account, false, "系统凭据存储不可用");
                    continue;
                };
                if $persist_enabled
                    && let Some($player) = $player.as_ref()
                    && $player.save_account_queue(&$queue_store).is_err()
                {
                    auth_view(&$events, &$account, false, "当前队列保存失败，请重试");
                    continue;
                }
                $cancel_qr!();
                $cancel_account_requests!();
                $auth_busy = true;
                auth_view(&$events, &$account, true, "正在切换账号…");
                let session = $transport.session();
                let $sender = $sender.clone();
                std::thread::spawn(move || {
                    let result =
                        auth::switch(&session, &CredentialStore::new(backend), &uid).map(Some);
                    let _ = $sender.send(Command::AuthDone(AuthOperation::Switch, result));
                });
            }
            Command::ForgetAccount(uid) => {
                if $auth_busy || !uid.parse::<i64>().is_ok_and(|id| id > 0) {
                    continue;
                }
                let Some(backend) = $secrets.clone() else {
                    auth_view(&$events, &$account, false, "系统凭据存储不可用");
                    continue;
                };
                $account_list_generation = $account_list_generation.wrapping_add(1);
                $auth_busy = true;
                auth_view(&$events, &$account, true, "正在移除账号…");
                let previous = $account.clone();
                let $sender = $sender.clone();
                std::thread::spawn(move || {
                    let result = CredentialStore::new(backend)
                        .forget(&uid)
                        .map(|()| previous)
                        .map_err(AuthError::Storage);
                    let _ = $sender.send(Command::AuthDone(AuthOperation::Forget, result));
                });
            }
            Command::RetryAccountCleanup => {
                if $auth_busy {
                    continue;
                }
                let Some(backend) = $secrets.clone() else {
                    auth_view(&$events, &$account, false, "系统凭据存储不可用");
                    continue;
                };
                $account_list_generation = $account_list_generation.wrapping_add(1);
                $auth_busy = true;
                auth_view(&$events, &$account, true, "正在清理凭据…");
                let previous = $account.clone();
                let $sender = $sender.clone();
                std::thread::spawn(move || {
                    let result = CredentialStore::new(backend)
                        .retry_cleanup()
                        .map(|()| previous)
                        .map_err(AuthError::Storage);
                    let _ = $sender.send(Command::AuthDone(AuthOperation::Cleanup, result));
                });
            }
            Command::RefreshAccounts => {
                if $auth_busy {
                    continue;
                }
                $account_list_generation = $account_list_generation.wrapping_add(1);
                let generation = $account_list_generation;
                if let Some(backend) = $secrets.clone() {
                    let $sender = $sender.clone();
                    std::thread::spawn(move || {
                        let store = CredentialStore::new(backend);
                        let result = store
                            .remembered()
                            .and_then(|entries| {
                                Ok(AccountList {
                                    entries: entries
                                        .into_iter()
                                        .map(|entry| AccountEntry {
                                            uid: entry.uid,
                                            nickname: entry.nickname,
                                            stale: entry.stale,
                                        })
                                        .collect(),
                                    cleanup_pending: store.has_pending_cleanup()?,
                                })
                            })
                            .map_err(|_| ());
                        let _ = $sender.send(Command::AccountsDone(generation, result));
                    });
                } else {
                    let _ = $events.send(Event::Accounts(AccountList::default()));
                }
            }
            Command::AccountsDone(generation, result)
                if generation == $account_list_generation && !$auth_busy =>
            {
                match result {
                    Ok(entries) => {
                        let _ = $events.send(Event::Accounts(entries));
                    }
                    Err(()) => {
                        let _ = $events.send(Event::Status("无法读取已记住的账号".into()));
                    }
                }
            }
            Command::AccountsDone(..) => {}
            #[cfg(windows)]
            Command::LegacyCredentialNotice => {
                let _ = $events.send(Event::Status(
                    "旧版账号导入失败，请检查旧文件及系统密钥库后重启重试；旧文件已保留".into(),
                ));
            }
            Command::AuthDone(operation, result) => {
                $auth_busy = false;
                let mut reset_scope = false;
                let message = match result {
                    Ok(next) => {
                        let changed = matches!(
                            operation,
                            AuthOperation::Login
                                | AuthOperation::SignOut
                                | AuthOperation::Expired
                                | AuthOperation::Switch
                        ) || $account.as_ref().map(|a| &a.uid)
                            != next.as_ref().map(|a| &a.uid);
                        $account = next;
                        reset_scope = changed;
                        if changed && let Some($player) = $player.as_mut() {
                            if $player
                                .restore_account_queue(&$queue_store, $preferences.restore_queue)
                                .is_err()
                            {
                                $persist_enabled = false;
                                let _ = $events
                                    .send(Event::Status("账号队列无法恢复，原文件已保留".into()));
                            } else {
                                $persist_enabled = $preferences.restore_queue;
                            }
                        }
                        match operation {
                            AuthOperation::Login => "登录成功",
                            AuthOperation::Restore if $account.is_some() => "已恢复登录",
                            AuthOperation::Expired => "登录已失效，请重新登录",
                            AuthOperation::Switch => "已切换账号",
                            AuthOperation::Forget => "已移除已记住的账号",
                            AuthOperation::Cleanup => "已清理待移除凭据",
                            _ => "未登录",
                        }
                    }
                    Err(AuthError::Storage(_)) if matches!(operation, AuthOperation::Expired) => {
                        $account = None;
                        reset_scope = true;
                        if let Some($player) = $player.as_mut() {
                            if $player
                                .restore_account_queue(&$queue_store, $preferences.restore_queue)
                                .is_err()
                            {
                                $persist_enabled = false;
                            } else {
                                $persist_enabled = $preferences.restore_queue;
                            }
                        }
                        "登录已失效，无法清除保存的凭据，请解锁系统密钥库"
                    }
                    Err(AuthError::Storage(CredentialError::Invalid))
                        if matches!(operation, AuthOperation::Switch) =>
                    {
                        "账号不可用或凭据已失效，请重新登录"
                    }
                    Err(AuthError::Storage(_)) => "系统凭据存储不可用，请解锁后重试",
                    Err(AuthError::SessionChanged) => "账号状态已变化，请重试",
                };
                if reset_scope {
                    $browser.clear();
                    let _ = $events.send(Event::Browse($browser.view()));
                    $query_generation = $query_generation.wrapping_add(1);
                    if let Some(task) = $search_task.take() {
                        task.abort();
                    }
                    $search_results.clear();
                    $query_text.clear();
                    $search_loading = false;
                    let _ = $events.send(Event::Search(SearchView {
                        generation: $query_generation,
                        tracks: Vec::new(),
                        has_more: false,
                        loading: false,
                    }));
                    if let Some(task) = $lyrics_task.take() {
                        task.abort();
                    }
                    $lyrics_key = None;
                    $lyrics_generation = $lyrics_generation.wrapping_add(1);
                    let _ = $events.send(Event::Lyrics(LyricsView::Empty));
                }
                auth_view(&$events, &$account, false, message);
                let _ = $sender.send(Command::RefreshAccounts);
            }
        ] $rest)
    };
}
