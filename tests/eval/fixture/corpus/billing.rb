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
