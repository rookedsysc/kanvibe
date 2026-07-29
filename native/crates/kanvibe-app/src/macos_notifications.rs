use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        Mutex,
        mpsc::{Receiver, RecvTimeoutError, Sender, channel},
    },
    task::{Context, Poll, Waker},
    thread::{self, JoinHandle},
    time::Duration,
};

use kanvibe_hooks::AppNotification;
use mac_usernotifications::{
    Error as MacNotificationError, Notification, NotificationResponse, request_auth,
};

use crate::{
    NativeNotificationActivationSink, NativeNotificationDeliveryStatus, NativeNotificationPlatform,
    native_diagnostic_line,
};

const RESPONSE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const RESPONSE_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_PENDING_DELIVERIES: usize = 100;
const MAX_PENDING_RESPONSES: usize = 100;

enum NotificationCommand {
    Deliver {
        notification: AppNotification,
        activation: NativeNotificationActivationSink,
    },
    Shutdown,
}

struct PendingDelivery {
    notification: AppNotification,
    activation: NativeNotificationActivationSink,
}

type AuthorizationFuture = Pin<Box<dyn Future<Output = Result<bool, MacNotificationError>> + Send>>;

type ResponseFuture = Pin<
    Box<
        dyn Future<
                Output = (
                    String,
                    NativeNotificationActivationSink,
                    Result<NotificationResponse, MacNotificationError>,
                ),
            > + Send,
    >,
>;

pub(crate) struct MacOsNotificationPlatform {
    sender: Sender<NotificationCommand>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl MacOsNotificationPlatform {
    pub(crate) fn start() -> std::io::Result<Self> {
        let (sender, receiver) = channel();
        let worker = thread::Builder::new()
            .name("kanvibe-macos-notifications".to_owned())
            .spawn(move || run_notification_worker(receiver))?;
        Ok(Self {
            sender,
            worker: Mutex::new(Some(worker)),
        })
    }
}

impl NativeNotificationPlatform for MacOsNotificationPlatform {
    fn deliver(
        &self,
        notification: &AppNotification,
        activation: NativeNotificationActivationSink,
    ) -> Result<NativeNotificationDeliveryStatus, String> {
        self.sender
            .send(NotificationCommand::Deliver {
                notification: notification.clone(),
                activation,
            })
            .map_err(|_| "macOS notification worker is unavailable".to_owned())?;
        Ok(NativeNotificationDeliveryStatus::Queued)
    }
}

impl Drop for MacOsNotificationPlatform {
    fn drop(&mut self) {
        let _ = self.sender.send(NotificationCommand::Shutdown);
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = worker.join();
        }
    }
}

fn run_notification_worker(receiver: Receiver<NotificationCommand>) {
    let mut authorization: Option<Result<bool, String>> = None;
    let mut authorization_future: Option<AuthorizationFuture> = None;
    let mut deliveries = VecDeque::<PendingDelivery>::new();
    let mut responses = Vec::<ResponseFuture>::new();

    loop {
        match receiver.recv_timeout(RESPONSE_POLL_INTERVAL) {
            Ok(NotificationCommand::Deliver {
                notification,
                activation,
            }) => {
                deliveries.push_back(PendingDelivery {
                    notification,
                    activation,
                });
                if deliveries.len() > MAX_PENDING_DELIVERIES {
                    deliveries.pop_front();
                    eprintln!(
                        "{}",
                        native_diagnostic_line(
                            "notification-delivery-capacity",
                            "notification-center",
                            "oldest queued macOS notification delivery was released",
                            None,
                        )
                    );
                }
                if authorization.is_none() && authorization_future.is_none() {
                    authorization_future = Some(Box::pin(request_auth()));
                }
            }
            Ok(NotificationCommand::Shutdown) => return,
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }

        poll_authorization(
            &mut authorization,
            &mut authorization_future,
            &mut deliveries,
            &mut responses,
        );
        poll_notification_responses(&mut responses);
    }
}

fn poll_authorization(
    authorization: &mut Option<Result<bool, String>>,
    authorization_future: &mut Option<AuthorizationFuture>,
    deliveries: &mut VecDeque<PendingDelivery>,
    responses: &mut Vec<ResponseFuture>,
) {
    if let Some(future) = authorization_future {
        let mut context = Context::from_waker(Waker::noop());
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            *authorization = Some(result.map_err(|error| error.to_string()));
            *authorization_future = None;
        }
    }
    let Some(authorization) = authorization.as_ref() else {
        return;
    };
    while let Some(delivery) = deliveries.pop_front() {
        let result = match authorization {
            Ok(false) => Ok(NativeNotificationDeliveryStatus::PermissionDenied),
            Err(error) => Err(error.clone()),
            Ok(true) => enqueue_notification(delivery.notification, delivery.activation, responses),
        };
        match result {
            Ok(NativeNotificationDeliveryStatus::PermissionDenied) => eprintln!(
                "{}",
                native_diagnostic_line(
                    "notification-permission-denied",
                    "notification-center",
                    "macOS notification permission is denied",
                    None,
                )
            ),
            Ok(_) => {}
            Err(error) => eprintln!(
                "{}",
                native_diagnostic_line(
                    "notification-delivery-error",
                    "notification-center",
                    &error,
                    None,
                )
            ),
        }
    }
}

fn enqueue_notification(
    notification: AppNotification,
    activation: NativeNotificationActivationSink,
    responses: &mut Vec<ResponseFuture>,
) -> Result<NativeNotificationDeliveryStatus, String> {
    let notification_id = notification.id.clone();
    let handle = Notification::new()
        .id(&notification.id)
        .title(&notification.title)
        .message(&notification.body)
        .default_sound()
        .timeout(RESPONSE_RETENTION)
        .send_blocking()
        .map_err(|error| error.to_string())?;
    let response_notification_id = notification_id.clone();
    responses.push(Box::pin(async move {
        let response = handle.response().await;
        (response_notification_id, activation, response)
    }));
    if responses.len() > MAX_PENDING_RESPONSES {
        std::mem::drop(responses.remove(0));
        eprintln!(
            "{}",
            native_diagnostic_line(
                "notification-response-capacity",
                "notification-center",
                "oldest macOS notification response observer was released",
                None,
            )
        );
    }
    Ok(NativeNotificationDeliveryStatus::Delivered)
}

fn poll_notification_responses(responses: &mut Vec<ResponseFuture>) {
    let mut index = responses.len();
    while index > 0 {
        index -= 1;
        let mut context = Context::from_waker(Waker::noop());
        if let Poll::Ready((notification_id, activation, response)) =
            responses[index].as_mut().poll(&mut context)
        {
            std::mem::drop(responses.swap_remove(index));
            match response {
                Ok(response) if response.is_default_action() => {
                    if let Err(error) = activation.activate(&notification_id) {
                        eprintln!(
                            "{}",
                            native_diagnostic_line(
                                "notification-activation-error",
                                "notification-center",
                                &error,
                                None,
                            )
                        );
                    }
                }
                Ok(_) => {}
                Err(error) => eprintln!(
                    "{}",
                    native_diagnostic_line(
                        "notification-response-error",
                        "notification-center",
                        &error.to_string(),
                        None,
                    )
                ),
            }
        }
    }
}
