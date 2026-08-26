class Widget
  def save(record)
    checked = record.validate
    persist(checked)
    checked
  end
end

class Gadget
  # The same method with every name changed: the method, the parameter, and
  # the local. Nothing a caller of `validate` or `persist` would notice.
  def store(item)
    verified = item.validate
    persist(verified)
    verified
  end
end

class Doodad
  # The same shape with one more thing done. Not a clone.
  def save(record)
    checked = record.validate
    audit(checked)
    persist(checked)
    checked
  end
end

class Gizmo
  # The same names, used in a different order. Ordinals are positional, so
  # this is a real change even though the same two locals appear.
  def save(record)
    checked = record.validate
    persist(record)
    checked
  end
end
