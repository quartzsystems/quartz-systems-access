// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Return codes of the sign-on server and of start-server requests: a
//! class in the high half, a code in the low half.

/// A return code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReturnCode(pub u32);

/// What kind of failure a return code is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Ok,
    /// The request was not valid.
    Request,
    /// About the user ID.
    User,
    /// About the password, or a new password's rules.
    Password,
    /// A general security error; the function was not done.
    Security,
    Other,
}

impl ReturnCode {
    pub const OK: ReturnCode = ReturnCode(0);
    pub const USER_UNKNOWN: ReturnCode = ReturnCode(0x0002_0001);
    pub const USER_DISABLED: ReturnCode = ReturnCode(0x0002_0002);
    pub const PASSWORD_INCORRECT: ReturnCode = ReturnCode(0x0003_000B);
    /// Incorrect, and the next incorrect attempt disables the profile.
    pub const PASSWORD_INCORRECT_LAST: ReturnCode = ReturnCode(0x0003_000C);
    pub const PASSWORD_EXPIRED: ReturnCode = ReturnCode(0x0003_000D);
    pub const PASSWORD_NONE: ReturnCode = ReturnCode(0x0003_0010);

    pub fn is_ok(self) -> bool {
        self.0 == 0
    }

    pub fn class(self) -> Class {
        if self.0 == 0 {
            return Class::Ok;
        }
        match self.0 >> 16 {
            1 => Class::Request,
            2 => Class::User,
            3 => Class::Password,
            4 => Class::Security,
            _ => Class::Other,
        }
    }

    /// Whether this rejects a new password for breaking one of the
    /// system's password rules. The user sees the system's own message for
    /// it; the client has no password rules of its own.
    pub fn is_password_rule(self) -> bool {
        matches!(
            self.0,
            0x0003_0001..=0x0003_000A | 0x0003_000F | 0x0003_0011..=0x0003_0013
        )
    }

    /// A short meaning, for logs and for when the system sent no message.
    pub fn meaning(self) -> &'static str {
        match self.0 {
            0 => "successful",
            0x0001_0001 => "exchange attributes request not valid",
            0x0001_0002 => "server ID not valid",
            0x0001_0003 => "request ID not valid",
            0x0001_0004 => "random seed not valid",
            0x0001_0005 => "random seed required for a password substitute",
            0x0001_0006 => "password encryption indicator not valid",
            0x0001_0007 => "user ID length not valid",
            0x0001_0008 => "password length not valid",
            0x0001_0009 => "client version not valid",
            0x0001_000A => "data stream level not valid",
            0x0001_000B => "retrieve sign-on information request not valid",
            0x0001_000C => "change password request not valid",
            0x0001_000D => "protected old password not valid",
            0x0001_000E => "protected new password not valid",
            0x0001_000F..=0x0001_0012 => "authentication token request not valid",
            0x0002_0001 => "user ID unknown",
            0x0002_0002 => "user profile disabled",
            0x0002_0003 => "user profile mismatch",
            0x0003_0001 => "new password longer than the maximum",
            0x0003_0002 => "new password shorter than the minimum",
            0x0003_0003 => "new password uses a character too often",
            0x0003_0004 => "new password has adjacent digits",
            0x0003_0005 => "new password repeats a character consecutively",
            0x0003_0006 => "new password was used before",
            0x0003_0007 => "new password must contain a digit",
            0x0003_0008 => "new password has a character that is not valid",
            0x0003_0009 => "new password is in the password dictionary",
            0x0003_000A => "new password contains the user ID",
            0x0003_000B => "password incorrect",
            0x0003_000C => "password incorrect; the next incorrect password disables the profile",
            0x0003_000D => "password expired",
            0x0003_000E => "password was saved in a form the system no longer accepts",
            0x0003_000F => "new password has a character in the same position as the old one",
            0x0003_0010 => "user profile has no password",
            0x0003_0011 => "password validation program rejected the password",
            0x0003_0012 => "password cannot be changed now",
            0x0003_0013 => "password value not valid",
            c if c >> 16 == 4 => "security error; the function was not done",
            _ => "unknown return code",
        }
    }
}

impl std::fmt::Display for ReturnCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:08X} ({})", self.0, self.meaning())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_and_meanings() {
        assert_eq!(ReturnCode::OK.class(), Class::Ok);
        assert_eq!(ReturnCode::USER_DISABLED.class(), Class::User);
        assert_eq!(ReturnCode::PASSWORD_EXPIRED.class(), Class::Password);
        assert_eq!(ReturnCode(0x0004_0007).class(), Class::Security);
        assert_eq!(ReturnCode(0x0001_0004).class(), Class::Request);
        assert_eq!(ReturnCode(0x0009_0000).class(), Class::Other);
        assert!(ReturnCode(0x0003_0002).is_password_rule());
        assert!(!ReturnCode::PASSWORD_INCORRECT.is_password_rule());
        assert_eq!(
            ReturnCode::PASSWORD_EXPIRED.to_string(),
            "0003000D (password expired)"
        );
        assert_eq!(
            ReturnCode(0x0004_00FF).meaning(),
            "security error; the function was not done"
        );
        assert_eq!(ReturnCode(0x0007_0001).meaning(), "unknown return code");
    }
}
