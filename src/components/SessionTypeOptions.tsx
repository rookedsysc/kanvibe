import { SessionType } from "@/entities/KanbanTask";

/**
 * 세션 타입 select에 들어가는 선택지.
 * 작업 생성, 브랜치 작업, 터미널 연결, 설정의 네 select가 같은 목록을 보여야 해서 한 곳에서 만든다.
 */
export default function SessionTypeOptions() {
  return (
    <>
      {Object.values(SessionType).map((sessionType) => (
        <option key={sessionType} value={sessionType}>
          {sessionType}
        </option>
      ))}
    </>
  );
}
