class Invoice
  def unpaid_for(customer)
    where(customer: customer, paid_at: nil).order(:created_at)
  end

  def settle!(paid_at)
    update!(paid_at: paid_at, state: "paid")
    Notifier.deliver(self)
  end

  def total_cents
    line_items.sum { |item| item.quantity * item.unit_cents }
  end

  def overdue?
    paid_at.nil? && due_on < Date.today
  end
end

class Receipt
  def outstanding(buyer)
    where(buyer: buyer, paid_at: nil).order(:created_at)
  end

  def total_cents
    entries.sum { |entry| entry.quantity * entry.unit_cents }
  end
end

class Dispatcher
  # Behaviour that no identifier reveals: the retry, the backoff, the cap.
  def send_reminder(invoice)
    attempts = 0
    begin
      attempts += 1
      Mailer.overdue(invoice).deliver_now
    rescue Net::SMTPError
      sleep(2**attempts)
      retry if attempts < 3
      raise
    end
  end

  # Behaviour that no identifier reveals: the result is cached after the
  # first call, so repeated calls cost nothing.
  def rates
    @rates ||= Currency.all.index_by(&:code)
  end

  # Behaviour that no identifier reveals: calling this twice is the same as
  # calling it once.
  def close(period)
    return period if period.closed_at
    period.update!(closed_at: Time.now)
    period
  end
end
