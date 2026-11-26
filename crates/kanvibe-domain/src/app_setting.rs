//! `app_settings` 테이블의 행. 앱 전역 설정을 키 하나에 문자열 값 하나로 보관한다.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSetting {
    pub id: String,
    pub key: String,
    /// 설정마다 `"true"`, 숫자 문자열, JSON 문자열 등 모양이 다르다. 해석은 설정 서비스가 맡는다
    pub value: String,
    pub created_at: String,
    pub updated_at: String,
}
