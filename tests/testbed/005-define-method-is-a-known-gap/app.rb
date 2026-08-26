module Alpha
  # Extracted. The interpolated-name case is the one trekr's extractor was
  # written for: the `each` binds `n` to two known literals, so both method
  # names are readable without guessing.
  [:one, :two].each do |n|
    define_method("#{n}_step") { 1 }
  end

  # NOT extracted, and this is the gap. A plain symbol argument is the
  # easiest possible case and it falls through — so `Alpha#plain` is invisible
  # to `--symbols`, to the index, and to everything downstream.
  define_method(:plain) { 1 }
end

class Widget
  def save(record)
    checked = record.validate
    persist(checked)
    checked
  end
end

class Gadget
  define_method(:save) do |record|
    checked = record.validate
    persist(checked)
    checked
  end
end
