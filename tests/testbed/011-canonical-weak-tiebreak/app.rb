class Ledger
  def settle(entry)
    amount = entry.total
    record(amount)
    amount
  end
end

module Billing
  module Legacy
    class Ledger
      def settle(entry)
        amount = entry.total
        record(amount)
        amount
      end
    end
  end
end
