import type { LegacyDataSummary } from "./localData";

export type LocalDataState = {
  legacyData: LegacyDataSummary | null;
  legacyExported: boolean;
  deleteOpen: boolean;
  deleteConfirmation: string;
  deleting: boolean;
  error: string | null;
  notice: string | null;
};

export type LocalDataAction =
  | { type: "legacy_exported" }
  | { type: "legacy_cleared" }
  | { type: "delete_opened" }
  | { type: "delete_closed" }
  | { type: "delete_confirmation_changed"; value: string }
  | { type: "delete_started" }
  | { type: "delete_succeeded"; nativeCleanup: boolean }
  | { type: "delete_failed"; error: string }
  | { type: "operation_failed"; error: string }
  | { type: "notice_cleared" };

export function createLocalDataState(
  legacyData: LegacyDataSummary | null,
): LocalDataState {
  return {
    legacyData,
    legacyExported: false,
    deleteOpen: false,
    deleteConfirmation: "",
    deleting: false,
    error: null,
    notice: null,
  };
}

export function localDataReducer(
  state: LocalDataState,
  action: LocalDataAction,
): LocalDataState {
  switch (action.type) {
    case "legacy_exported":
      return { ...state, legacyExported: true, error: null };
    case "legacy_cleared":
      return {
        ...state,
        legacyData: null,
        legacyExported: false,
        notice: "Saved legacy conversations were exported and removed.",
      };
    case "delete_opened":
      return {
        ...state,
        deleteOpen: true,
        deleteConfirmation: "",
        error: null,
      };
    case "delete_closed":
      return state.deleting
        ? state
        : {
            ...state,
            deleteOpen: false,
            deleteConfirmation: "",
            error: null,
          };
    case "delete_confirmation_changed":
      return { ...state, deleteConfirmation: action.value };
    case "delete_started":
      return { ...state, deleting: true, error: null };
    case "delete_succeeded":
      return {
        ...createLocalDataState(null),
        notice: action.nativeCleanup
          ? "All Bridge-owned local data was deleted."
          : "Browser data was deleted. Native cleanup is not available in this build.",
      };
    case "delete_failed":
      return { ...state, deleting: false, error: action.error };
    case "operation_failed":
      return { ...state, error: action.error };
    case "notice_cleared":
      return { ...state, notice: null };
  }
}
