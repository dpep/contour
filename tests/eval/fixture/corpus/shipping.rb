# A guard clause and the four methods that call it. The shape a field report
# found: the one method that IMPLEMENTS the check is out-ranked by methods that
# merely narrate calling it. See docs/PLAN.md, "The guard-clause case".
class Shipment
  def ensure_open!
    raise Frozen, "shipment is finalized" if finalized_at
  end

  def add_parcel(parcel)
    ensure_open!
    parcels << parcel
    recalculate_weight
  end

  def remove_parcel(parcel)
    ensure_open!
    parcels.delete(parcel)
    recalculate_weight
  end

  def relabel(name)
    ensure_open!
    update!(label: name)
  end

  def reroute(depot)
    ensure_open!
    update!(depot: depot)
    Notifier.deliver(self)
  end
end
