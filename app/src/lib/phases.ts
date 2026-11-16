import type { Board, Phase, Question } from "@/api";

/**
 * Phases to list: one with no card and no open question is hidden, so a default or planned
 * phase nobody used does not clutter the picker. A board with no work at all keeps its phases,
 * and the current phase is always kept so the board never shows an empty list.
 */
export function phasesWithWork(board: Board, questions: Question[]): Phase[] {
  const used = new Set<string>();
  for (const lane of board.lanes) for (const card of lane.cards) used.add(card.phase_id);
  for (const question of questions) {
    if (question.board_id === board.id && question.status === "open") used.add(question.phase_id);
  }
  if (used.size === 0) return board.phases;
  return board.phases.filter((phase) => used.has(phase.id) || phase.id === board.current_phase_id);
}
