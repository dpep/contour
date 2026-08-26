class Warehouse
  def reserve(sku, count)
    raise OutOfStock if available(sku) < count
    holds.create!(sku: sku, count: count, expires_at: 15.minutes.from_now)
  end

  def available(sku)
    stock_level(sku) - holds.active.where(sku: sku).sum(:count)
  end

  def restock(sku, count)
    levels.find_or_initialize_by(sku: sku).increment!(:on_hand, count)
  end
end

# Copy-pasted from Warehouse when the depot feature was split out.
class Depot
  def available(sku)
    stock_level(sku) - holds.active.where(sku: sku).sum(:count)
  end

  def reserve(sku, count)
    raise OutOfStock if available(sku) < count
    holds.create!(sku: sku, count: count, expires_at: 15.minutes.from_now)
  end
end
