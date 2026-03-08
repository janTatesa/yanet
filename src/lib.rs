use core::{error::Error as StdError, fmt, panic, result};
use std::{
    borrow::Cow,
    process::{ExitCode, Termination},
};

#[cfg(feature = "colors")]
use owo_colors::OwoColorize;

pub struct Report(ReportInner, &'static panic::Location<'static>);

#[derive(Debug)]
pub enum ReportInner {
    Msg(Cow<'static, str>),
    ErrorWrapped(Box<dyn StdError + Send + Sync>, Cow<'static, str>),
    Error(Box<dyn StdError + Send + Sync>),
    Report(Box<Report>, Cow<'static, str>),
}

impl Report {
    #[track_caller]
    pub fn new(str: impl Into<Cow<'static, str>>) -> Self {
        Self(ReportInner::Msg(str.into()), panic::Location::caller())
    }

    #[track_caller]
    pub fn wrap(self, msg: impl Into<Cow<'static, str>>) -> Self {
        Self(
            ReportInner::Report(Box::new(self), msg.into()),
            panic::Location::caller(),
        )
    }

    pub fn location(&self) -> panic::Location<'static> {
        *self.1
    }

    pub fn message(&self) -> Cow<'_, str> {
        match &self.0 {
            ReportInner::Msg(cow)
            | ReportInner::ErrorWrapped(_, cow)
            | ReportInner::Report(_, cow) => Cow::Borrowed(cow),
            ReportInner::Error(error) => error.to_string().into(),
        }
    }

    pub fn iter(&self) -> ReportIter<'_> {
        ReportIter {
            inner: Some(ReportIterInner::Report(self)),
        }
    }

    #[cfg(not(feature = "colors"))]
    fn debug_inner(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(
            f,
            "{}\n  in {}:{}:{}",
            self.message(),
            self.1.file(),
            self.1.line(),
            self.1.column()
        )?;
        let mut causes = self.iter().peekable();
        causes.next();
        if causes.peek().is_some() {
            f.write_str("\nCaused by:")?;
            for (msg, location) in causes {
                match location {
                    Some(location) => write!(
                        f,
                        "\n  {msg}\n    in {}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    )?,
                    None => f.write_str(&msg)?,
                };
            }
        };
        Ok(())
    }

    #[cfg(feature = "colors")]
    fn debug_inner(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(
            f,
            "{}\n  in {}{}{}{}{}",
            self.message().red(),
            self.1.file().magenta(),
            ":".bright_black(),
            self.1.line().magenta(),
            ":".bright_black(),
            self.1.column().magenta()
        )?;
        let mut causes = self.iter().peekable();
        causes.next();
        if causes.peek().is_some() {
            f.write_str("\nCaused by:")?;
            for (msg, location) in causes {
                match location {
                    Some(location) => write!(
                        f,
                        "\n  {}\n    in {}{}{}{}{}",
                        msg.red(),
                        location.file().magenta(),
                        ":".bright_black(),
                        location.line().magenta(),
                        ":".bright_black(),
                        location.column().magenta()
                    )?,
                    None => f.write_str(&msg)?,
                };
            }
        };
        Ok(())
    }
}

pub struct ReportIter<'a> {
    inner: Option<ReportIterInner<'a>>,
}

#[derive(Clone, Copy)]
enum ReportIterInner<'a> {
    Error(&'a dyn StdError),
    Report(&'a Report),
}

impl<'a> Iterator for ReportIter<'a> {
    type Item = (Cow<'a, str>, Option<panic::Location<'static>>);

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner? {
            ReportIterInner::Error(error) => {
                let cause = (error.to_string().into(), None);
                self.inner = error.source().map(ReportIterInner::Error);
                Some(cause)
            }
            ReportIterInner::Report(Report(ReportInner::Msg(msg), location)) => {
                self.inner = None;
                Some(((&**msg).into(), Some(**location)))
            }
            ReportIterInner::Report(Report(ReportInner::Report(inner_report, msg), location)) => {
                self.inner = Some(ReportIterInner::Report(inner_report));
                Some(((&**msg).into(), Some(**location)))
            }
            ReportIterInner::Report(Report(ReportInner::ErrorWrapped(error, msg), location)) => {
                self.inner = Some(ReportIterInner::Error(&**error));
                Some(((&**msg).into(), Some(**location)))
            }
            ReportIterInner::Report(Report(ReportInner::Error(error), location)) => {
                self.inner = error.source().map(ReportIterInner::Error);
                Some((error.to_string().into(), Some(**location)))
            }
        }
    }
}

impl fmt::Debug for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if std::env::var("YANET_DEBUG_RAW").is_ok_and(|var| var == "1") {
            f.debug_tuple("Report")
                .field(&self.0)
                .field(&self.1)
                .finish()
        } else {
            self.debug_inner(f)?;

            Ok(())
        }
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}\n  at {}:{}:{}",
            self.message(),
            self.1.file(),
            self.1.line(),
            self.1.column()
        )
    }
}

impl<E> From<E> for Report
where
    E: StdError + 'static + Send + Sync,
{
    #[track_caller]
    fn from(val: E) -> Report {
        Report(ReportInner::Error(Box::new(val)), panic::Location::caller())
    }
}

pub type Result<T, E = Report> = result::Result<T, E>;

#[macro_export]
macro_rules! yanet {
    ($err:expr $(,)?) => ({
        Report::new(err)
    });
    ($fmt:literal, $($arg:tt)*) => {
        Report::new(format!($fmt, $($arg)*))
    };
    ($err:expr, $msg:expr) => {
        $err.wrap($msg)
    };
}

pub trait ErrorExt {
    fn wrap(self, s: impl Into<Cow<'static, str>>) -> Report;
}

impl<E: StdError + Send + Sync + 'static> ErrorExt for E {
    #[track_caller]
    fn wrap(self, s: impl Into<Cow<'static, str>>) -> Report {
        Report(
            ReportInner::ErrorWrapped(Box::new(self), s.into()),
            panic::Location::caller(),
        )
    }
}

pub trait ResultExt<T, E> {
    fn wrap_err(self, s: impl Into<Cow<'static, str>>) -> Result<T, Report>;
    fn wrap_err_with<F: FnOnce(&E) -> O, O: Into<Cow<'static, str>>>(
        self,
        f: F,
    ) -> Result<T, Report>;
}

impl<T, E: StdError + Send + Sync + 'static> ResultExt<T, E> for Result<T, E> {
    #[track_caller]
    fn wrap_err(self, s: impl Into<Cow<'static, str>>) -> Result<T, Report> {
        self.map_err(|e| {
            Report(
                ReportInner::ErrorWrapped(Box::new(e), s.into()),
                panic::Location::caller(),
            )
        })
    }

    #[track_caller]
    fn wrap_err_with<F: FnOnce(&E) -> O, O: Into<Cow<'static, str>>>(
        self,
        f: F,
    ) -> Result<T, Report> {
        match self {
            Ok(t) => Ok(t),
            Err(e) => Err({
                let s = f(&e);
                Report(
                    ReportInner::ErrorWrapped(Box::new(e), s.into()),
                    panic::Location::caller(),
                )
            }),
        }
    }
}

impl<T> ResultExt<T, Report> for Result<T, Report> {
    #[track_caller]
    fn wrap_err(self, s: impl Into<Cow<'static, str>>) -> Result<T, Report> {
        match self {
            Ok(t) => Ok(t),
            Err(e) => Err({
                Report(
                    ReportInner::Report(Box::new(e), s.into()),
                    panic::Location::caller(),
                )
            }),
        }
    }

    #[track_caller]
    fn wrap_err_with<F: FnOnce(&Report) -> O, O: Into<Cow<'static, str>>>(
        self,
        f: F,
    ) -> Result<T, Report> {
        match self {
            Ok(t) => Ok(t),
            Err(e) => Err({
                let s = f(&e);
                Report(
                    ReportInner::Report(Box::new(e), s.into()),
                    panic::Location::caller(),
                )
            }),
        }
    }
}

pub trait OptionExt<T> {
    fn ok_or_yanet(self, s: impl Into<Cow<'static, str>>) -> Result<T, Report>;
    fn ok_or_else_yanet<O: Into<Cow<'static, str>>>(
        self,
        f: impl FnOnce() -> O,
    ) -> Result<T, Report>;
}

impl<T> OptionExt<T> for Option<T> {
    #[track_caller]
    fn ok_or_yanet(self, s: impl Into<Cow<'static, str>>) -> Result<T, Report> {
        match self {
            Some(v) => Ok(v),
            None => Err(Report(
                ReportInner::Msg(s.into()),
                panic::Location::caller(),
            )),
        }
    }

    #[track_caller]
    fn ok_or_else_yanet<O: Into<Cow<'static, str>>>(
        self,
        f: impl FnOnce() -> O,
    ) -> Result<T, Report> {
        match self {
            Some(v) => Ok(v),
            None => {
                let str = f();
                Err(Report(
                    ReportInner::Msg(str.into()),
                    panic::Location::caller(),
                ))
            }
        }
    }
}

impl Termination for Report {
    fn report(self) -> std::process::ExitCode {
        ExitCode::FAILURE
    }
}
