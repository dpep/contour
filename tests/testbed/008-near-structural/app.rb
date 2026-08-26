class Alpha
  def render(rows, width)
    raise Empty if rows.none?
    labelled = rows.map { |row| format(row.name, row.size) }
    trimmed = labelled.map { |label| label.slice(0, width) }
    grouped = trimmed.group_by { |label| label.chars.first }
    grouped.each_value { |group| group.sort! }
    grouped.keys.sort.map { |key| [key, grouped[key]] }
  end
end

class Beta
  # Copy-paste, then tweaked: one extra call in one chain. Not an exact clone,
  # and not unrelated either — the whole reason the near tier exists.
  def render(rows, width)
    raise Empty if rows.none?
    labelled = rows.map { |row| format(row.name, row.size) }
    trimmed = labelled.map { |label| label.slice(0, width) }
    grouped = trimmed.group_by { |label| label.chars.first }
    grouped.each_value { |group| group.uniq!.sort! }
    grouped.keys.sort.map { |key| [key, grouped[key]] }
  end
end

class Gamma
  # Reuses several of the same method names, does different work.
  def render(rows, width)
    raise Empty if rows.none?
    @cache ||= rows.each_with_object({}) { |row, acc| acc[row.name] = row.size }
    @cache.select { |_, size| size < width }.keys.sort
  end
end
