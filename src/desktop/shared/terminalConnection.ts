/**
 * 원격 Herdr 서버를 교체하느라 터미널을 닫을 때 화면에 보내는 닫힘 사유.
 * 이 사유로 닫힌 화면은 오류를 그리지 않고 곧바로 다시 연결한다. 교체가 끝날 때까지 다시 열기는 main이 기다린다.
 */
export const HERDR_SERVER_REPLACED_CLOSE_REASON = "kanvibe:herdr-server-replaced";
