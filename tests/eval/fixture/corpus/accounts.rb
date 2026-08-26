class Account
  def authenticate(password)
    return false unless active?
    Digest::SHA256.hexdigest(password + salt) == password_digest
  end

  def lock!(reason)
    update!(locked_at: Time.now, lock_reason: reason)
    AuditLog.record(self, "locked")
  end

  def active?
    locked_at.nil? && confirmed_at.present?
  end

  def display_name
    [first_name, last_name].compact.join(" ")
  end
end

class Session
  def authenticate(token)
    return false unless active?
    Digest::SHA256.hexdigest(token + salt) == token_digest
  end

  def expire!(reason)
    update!(expired_at: Time.now, expiry_reason: reason)
    AuditLog.record(self, "expired")
  end
end
